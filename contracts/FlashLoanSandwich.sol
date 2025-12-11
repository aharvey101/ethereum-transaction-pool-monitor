// SPDX-License-Identifier: MIT
pragma solidity ^0.8.19;

import "@openzeppelin/contracts/security/ReentrancyGuard.sol";
import "@openzeppelin/contracts/access/Ownable.sol";

interface IBalancerVault {
    function flashLoan(
        address recipient,
        address[] memory tokens,
        uint256[] memory amounts,
        bytes memory userData
    ) external;
}

interface IERC20 {
    function transfer(address to, uint256 amount) external returns (bool);
    function transferFrom(address from, address to, uint256 amount) external returns (bool);
    function balanceOf(address account) external view returns (uint256);
    function approve(address spender, uint256 amount) external returns (bool);
}

interface IWETH {
    function deposit() external payable;
    function withdraw(uint256 amount) external;
}

interface IUniswapV2Router {
    function swapExactETHForTokens(
        uint256 amountOutMin,
        address[] calldata path,
        address to,
        uint256 deadline
    ) external payable returns (uint256[] memory amounts);
    
    function swapExactTokensForETH(
        uint256 amountIn,
        uint256 amountOutMin,
        address[] calldata path,
        address to,
        uint256 deadline
    ) external returns (uint256[] memory amounts);
    
    function getAmountsOut(uint256 amountIn, address[] calldata path)
        external view returns (uint256[] memory amounts);
}

/**
 * @title FlashLoanSandwich
 * @notice Executes sandwich attacks using Balancer V2 flash loans within Flashbots bundles
 * @dev This contract receives flash loans and executes frontrun/backrun swaps atomically
 */
contract FlashLoanSandwich is ReentrancyGuard, Ownable {
    
    // Constants
    address public constant BALANCER_VAULT = 0xBA12222222228d8Ba445958a75a0704d566BF2C8;
    address public constant WETH = 0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2;
    address public constant UNISWAP_V2_ROUTER = 0x7a250d5630B4cF539739dF2C5dAcb4c659F2488D;
    
    // State variables
    uint256 public minProfitBasisPoints = 50; // 0.5% minimum profit
    uint256 public maxSlippageBasisPoints = 200; // 2% max slippage
    
    // Events
    event SandwichExecuted(
        address indexed token,
        uint256 flashLoanAmount,
        uint256 profit,
        uint256 gasUsed
    );
    
    event FlashLoanReceived(
        address indexed token,
        uint256 amount
    );
    
    // Structs
    struct SandwichParams {
        address tokenIn;
        address tokenOut; 
        uint256 frontrunAmountIn;
        uint256 expectedBackrunAmountIn;
        uint256 minProfitWei;
        uint256 deadline;
        address[] frontrunPath;
        address[] backrunPath;
    }
    
    /**
     * @notice Execute sandwich attack with flash loan
     * @param flashLoanAmount Amount of WETH to borrow
     * @param params Sandwich execution parameters
     */
    function executeSandwichWithFlashLoan(
        uint256 flashLoanAmount,
        SandwichParams calldata params
    ) external nonReentrant onlyOwner {
        
        // Validate parameters
        require(flashLoanAmount > 0, "Invalid flash loan amount");
        require(params.frontrunAmountIn <= flashLoanAmount, "Frontrun amount exceeds flash loan");
        require(block.timestamp <= params.deadline, "Deadline exceeded");
        
        // Prepare flash loan
        address[] memory tokens = new address[](1);
        uint256[] memory amounts = new uint256[](1);
        tokens[0] = WETH;
        amounts[0] = flashLoanAmount;
        
        // Encode sandwich parameters for flash loan callback
        bytes memory userData = abi.encode(params);
        
        emit FlashLoanReceived(WETH, flashLoanAmount);
        
        // Initiate flash loan - this will call receiveFlashLoan
        IBalancerVault(BALANCER_VAULT).flashLoan(
            address(this),
            tokens,
            amounts,
            userData
        );
    }
    
    /**
     * @notice Balancer V2 flash loan callback
     * @param tokens Array of token addresses (should contain WETH)
     * @param amounts Array of borrowed amounts
     * @param feeAmounts Array of fee amounts (0 for Balancer V2)
     * @param userData Encoded sandwich parameters
     */
    function receiveFlashLoan(
        address[] memory tokens,
        uint256[] memory amounts,
        uint256[] memory feeAmounts,
        bytes memory userData
    ) external {
        require(msg.sender == BALANCER_VAULT, "Only Balancer Vault can call this");
        require(tokens[0] == WETH, "Only WETH flash loans supported");
        
        uint256 flashLoanAmount = amounts[0];
        uint256 flashLoanFee = feeAmounts[0]; // Should be 0 for Balancer V2
        
        // Decode sandwich parameters
        SandwichParams memory params = abi.decode(userData, (SandwichParams));
        
        // Convert WETH to ETH for frontrun
        IWETH(WETH).withdraw(params.frontrunAmountIn);
        
        uint256 initialWETHBalance = IERC20(WETH).balanceOf(address(this));
        uint256 initialETHBalance = address(this).balance;
        
        // STEP 1: Execute frontrun swap (ETH → Token)
        uint256[] memory frontrunAmounts = IUniswapV2Router(UNISWAP_V2_ROUTER)
            .swapExactETHForTokens{value: params.frontrunAmountIn}(
                0, // Accept any amount of tokens out
                params.frontrunPath,
                address(this),
                params.deadline
            );
        
        uint256 tokensReceived = frontrunAmounts[frontrunAmounts.length - 1];
        
        // NOTE: At this point, victim transaction should execute between
        // our frontrun and backrun within the Flashbots bundle
        
        // STEP 2: Execute backrun swap (Token → ETH)
        // Approve tokens for router
        IERC20(params.tokenOut).approve(UNISWAP_V2_ROUTER, tokensReceived);
        
        uint256[] memory backrunAmounts = IUniswapV2Router(UNISWAP_V2_ROUTER)
            .swapExactTokensForETH(
                tokensReceived,
                0, // Accept any amount of ETH out
                params.backrunPath,
                address(this),
                params.deadline
            );
        
        uint256 ethReceived = backrunAmounts[backrunAmounts.length - 1];
        
        // Convert ETH back to WETH for repayment
        IWETH(WETH).deposit{value: ethReceived}();
        
        // Calculate profit
        uint256 finalWETHBalance = IERC20(WETH).balanceOf(address(this));
        uint256 totalWETH = finalWETHBalance + address(this).balance; // In case some ETH remains
        
        // Convert any remaining ETH to WETH
        if (address(this).balance > 0) {
            IWETH(WETH).deposit{value: address(this).balance}();
            totalWETH = IERC20(WETH).balanceOf(address(this));
        }
        
        uint256 repaymentAmount = flashLoanAmount + flashLoanFee;
        require(totalWETH >= repaymentAmount, "Insufficient funds to repay flash loan");
        
        // Repay flash loan
        IERC20(WETH).transfer(BALANCER_VAULT, repaymentAmount);
        
        // Calculate and validate profit
        uint256 profit = totalWETH - repaymentAmount;
        uint256 minProfit = (flashLoanAmount * minProfitBasisPoints) / 10000;
        require(profit >= minProfit, "Profit below minimum threshold");
        require(profit >= params.minProfitWei, "Profit below expected minimum");
        
        // Transfer profit to owner
        if (profit > 0) {
            IERC20(WETH).transfer(owner(), profit);
        }
        
        emit SandwichExecuted(
            params.tokenOut,
            flashLoanAmount,
            profit,
            gasleft()
        );
    }
    
    /**
     * @notice Emergency function to withdraw any stuck tokens
     */
    function emergencyWithdraw(address token, uint256 amount) external onlyOwner {
        if (token == address(0)) {
            payable(owner()).transfer(amount);
        } else {
            IERC20(token).transfer(owner(), amount);
        }
    }
    
    /**
     * @notice Update minimum profit threshold
     */
    function setMinProfitBasisPoints(uint256 _minProfitBasisPoints) external onlyOwner {
        require(_minProfitBasisPoints <= 1000, "Max 10% minimum profit");
        minProfitBasisPoints = _minProfitBasisPoints;
    }
    
    /**
     * @notice Update maximum slippage tolerance
     */
    function setMaxSlippageBasisPoints(uint256 _maxSlippageBasisPoints) external onlyOwner {
        require(_maxSlippageBasisPoints <= 1000, "Max 10% slippage");
        maxSlippageBasisPoints = _maxSlippageBasisPoints;
    }
    
    // Allow contract to receive ETH
    receive() external payable {}
}