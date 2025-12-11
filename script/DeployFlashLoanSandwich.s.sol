// SPDX-License-Identifier: MIT
pragma solidity ^0.8.19;

import "forge-std/Script.sol";
import "../contracts/FlashLoanSandwich.sol";

contract DeployFlashLoanSandwich is Script {
    function run() external {
        // Get deployer private key from environment
        uint256 deployerPrivateKey = vm.envUint("PRIVATE_KEY");
        
        // Start broadcasting transactions
        vm.startBroadcast(deployerPrivateKey);
        
        // Deploy the FlashLoanSandwich contract
        FlashLoanSandwich sandwich = new FlashLoanSandwich();
        
        console.log("FlashLoanSandwich deployed at:", address(sandwich));
        console.log("Deployer address:", vm.addr(deployerPrivateKey));
        console.log("Contract owner:", sandwich.owner());
        
        vm.stopBroadcast();
    }
}