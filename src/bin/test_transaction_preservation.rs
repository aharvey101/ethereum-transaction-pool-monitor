use anyhow::Result;
use std::collections::VecDeque;

#[derive(Clone, Debug)]
struct MockTransaction {
    id: usize,
    is_pending: bool,
}

fn simulate_transaction_update(
    current_transactions: &mut VecDeque<MockTransaction>,
    new_pending_transactions: Vec<MockTransaction>
) {
    println!("📊 Before update: {} transactions", current_transactions.len());
    
    // Simulate the preservation logic from main.rs
    let mut new_transactions = VecDeque::new();
    
    // Add new pending transactions (limit to 1000)
    for tx in new_pending_transactions {
        new_transactions.push_back(tx);
        if new_transactions.len() > 5 { // Use 5 for testing instead of 1000
            new_transactions.pop_front();
        }
    }
    
    println!("📥 Added {} new pending transactions", new_transactions.len());
    
    // Preserve historical transactions if we have them
    if current_transactions.len() > 5 {
        // Keep historical transactions (everything after position 5)
        for i in 5..current_transactions.len() {
            if let Some(historical_tx) = current_transactions.get(i) {
                new_transactions.push_back(historical_tx.clone());
            }
        }
        
        let preserved = current_transactions.len() - 5;
        println!("🏛️  Preserved {} historical transactions", preserved);
    }
    
    *current_transactions = new_transactions;
    
    let pending_count = current_transactions.len().min(5);
    let historical_count = current_transactions.len().saturating_sub(5);
    
    println!("✅ After update: {} total ({} pending, {} historical)", 
        current_transactions.len(), pending_count, historical_count);
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("🧪 Testing Transaction Preservation Logic...\n");
    
    let mut transactions = VecDeque::new();
    
    // Scenario 1: Initial load with pending transactions
    println!("🔄 Scenario 1: Initial pending transaction load");
    let initial_pending = (1..=5).map(|i| MockTransaction { id: i, is_pending: true }).collect();
    simulate_transaction_update(&mut transactions, initial_pending);
    println!();
    
    // Scenario 2: Load historical transactions (simulating pagination)
    println!("🔄 Scenario 2: Load historical transactions via pagination");
    for i in 101..=105 {
        transactions.push_back(MockTransaction { id: i, is_pending: false });
    }
    println!("📜 Manually added 5 historical transactions (simulating pagination)");
    println!("📊 Total after pagination: {}", transactions.len());
    println!();
    
    // Scenario 3: New pending transactions arrive (should preserve historical)
    println!("🔄 Scenario 3: New pending transactions arrive");
    let new_pending = (6..=10).map(|i| MockTransaction { id: i, is_pending: true }).collect();
    simulate_transaction_update(&mut transactions, new_pending);
    println!();
    
    // Scenario 4: Another round of new pending (should still preserve historical)
    println!("🔄 Scenario 4: Another batch of pending transactions");
    let more_pending = (11..=15).map(|i| MockTransaction { id: i, is_pending: true }).collect();
    simulate_transaction_update(&mut transactions, more_pending);
    println!();
    
    // Verify final state
    println!("🔍 Final verification:");
    println!("First 5 transactions (should be pending 11-15):");
    for i in 0..5.min(transactions.len()) {
        let tx = &transactions[i];
        println!("  {}. ID={} pending={}", i+1, tx.id, tx.is_pending);
    }
    
    if transactions.len() > 5 {
        println!("Historical transactions (should be 101-105):");
        for i in 5..transactions.len() {
            let tx = &transactions[i];
            println!("  {}. ID={} pending={}", i+1, tx.id, tx.is_pending);
        }
    }
    
    println!("\n✅ Transaction preservation test completed!");
    
    // Expected result: transactions 11-15 (pending) + 101-105 (historical)
    let expected_pending = transactions.iter().take(5).all(|tx| tx.is_pending);
    let expected_historical = transactions.iter().skip(5).all(|tx| !tx.is_pending);
    
    if expected_pending && expected_historical {
        println!("✅ SUCCESS: Historical transactions preserved correctly!");
    } else {
        println!("❌ FAIL: Historical transactions not preserved correctly");
    }
    
    Ok(())
}