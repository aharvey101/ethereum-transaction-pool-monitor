mod app;
mod eth_client;
mod ui;

use app::AppState;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() {
    // Setup terminal
    if let Err(e) = enable_raw_mode() {
        eprintln!("Failed to enable raw mode: {}", e);
        return;
    }

    let mut stdout = io::stdout();
    if let Err(e) = execute!(stdout, EnterAlternateScreen, EnableMouseCapture) {
        eprintln!("Failed to setup terminal: {}", e);
        return;
    }

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = match Terminal::new(backend) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Failed to create terminal: {}", e);
            return;
        }
    };

    // Get RPC URL from environment or use default
    let rpc_url = std::env::var("ETH_RPC_URL").unwrap_or_else(|_| "http://192.168.0.14:8545".to_string());

    // Create app state
    let mut app = match AppState::new(&rpc_url).await {
        Ok(app) => app,
        Err(e) => {
            let _ = disable_raw_mode();
            let _ = execute!(
                terminal.backend_mut(),
                LeaveAlternateScreen,
                DisableMouseCapture
            );
            let _ = terminal.show_cursor();
            eprintln!("Failed to connect to Ethereum node at {}: {}", rpc_url, e);
            eprintln!("Make sure your local Ethereum node is running.");
            eprintln!("You can set the RPC URL with: export ETH_RPC_URL=http://your-rpc-url:port");
            return;
        }
    };

    // Spawn task to update transactions periodically
    let rpc_url_clone = rpc_url.clone();
    let _update_handle = tokio::spawn(async move {
        if let Ok(update_client) = eth_client::EthereumClient::new(&rpc_url_clone).await {
            loop {
                sleep(Duration::from_secs(5)).await;
                let _ = update_client.get_pending_transactions().await;
            }
        }
    });

    // Run the main loop
    let _ = run_app(&mut terminal, &mut app).await;

    // Restore terminal
    let _ = disable_raw_mode();
    let _ = execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    );
    let _ = terminal.show_cursor();
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut AppState,
) -> io::Result<()> {
    // Initial update
    let _ = app.update_transactions().await;

    loop {
        // Get terminal size for dynamic row calculation
        let terminal_height = terminal.size()?.height as usize;
        let available_rows = terminal_height.saturating_sub(7); // 3 for header, 3 for footer, 1 margin

        // Draw UI
        terminal.draw(|f| ui::draw(f, app))?;

        // Handle input with timeout to allow periodic updates
        if crossterm::event::poll(Duration::from_millis(500))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => {
                        app.is_running = false;
                        break;
                    }
                    KeyCode::Up => {
                        app.select_previous(available_rows);
                    }
                    KeyCode::Down => {
                        app.select_next(available_rows);
                    }
                    KeyCode::PageUp => {
                        app.scroll_up(5);
                    }
                    KeyCode::PageDown => {
                        app.scroll_down(5, available_rows);
                    }
                    _ => {}
                }
            }
        } else {
            // Periodically update transactions
            let _ = app.update_transactions().await;
        }
    }

    Ok(())
}
