use crate::{TcpIncoming, TcpOutgoing};

use anyhow::Result;
use hex;
use rand_core::OsRng;
use round_based::MpcParty;
use sha2::Sha256;
use std::convert::TryInto;
use tokio::net::TcpStream;

use givre::ciphersuite::AdditionalEntropy;
use givre::generic_ec::{EncodedScalar, NonZero, SecretScalar, curves::Ed25519};
use givre::key_share::DirtyKeyShare;
use givre::keygen::{ExecutionId, ThresholdMsg, keygen};
use givre::keygen::{key_share::Valid, security_level::SecurityLevel128};

use solana_pubkey::Pubkey;
use solana_rpc_client::rpc_client::RpcClient;

type KeygenMsg = ThresholdMsg<Ed25519, SecurityLevel128, Sha256>;

/// Runs the DKG protocol for this participant and returns the generated private share.
pub async fn generate_private_share(
    std_stream_dkg: std::net::TcpStream,
    id: u64,
    session: &'static [u8],
) -> Result<Valid<DirtyKeyShare<Ed25519>>> {
    // Convert std streams to tokio streams
    let reader_stream_dkg = TcpStream::from_std(std_stream_dkg.try_clone()?)?;
    let writer_stream_dkg = TcpStream::from_std(std_stream_dkg)?;

    let incoming = TcpIncoming::<KeygenMsg>::new(reader_stream_dkg, id);
    let outgoing = TcpOutgoing::<KeygenMsg>::new(writer_stream_dkg, id);

    // Initialize builder for 2-of-2 threshold (adjust as needed)
    let eid = ExecutionId::new(session); // get this from the caller
    let builder = keygen::<Ed25519>(eid, id as u16, 2).set_threshold(2);
    let mut rng = OsRng;

    // Start MPC party
    let party = MpcParty::connected((incoming, outgoing));

    println!("[DKG] Starting DKG for participant {}", id);
    let valid_share = builder.start(&mut rng, party).await?;
    println!("[DKG] DKG completed for participant {}", id);

    // Print out key info
    let shared_public_key = valid_share.shared_public_key();
    println!(
        "[DKG] Shared Public Key (hex): {}",
        hex::encode(shared_public_key.to_bytes(true))
    );

    let private_share = &valid_share.x;
    let encoded: EncodedScalar<Ed25519> = <NonZero<SecretScalar<Ed25519>> as AdditionalEntropy<
        givre::ciphersuite::Ed25519,
    >>::to_bytes(private_share);
    let private_bytes: [u8; 32] = encoded.as_ref().try_into().unwrap();
    println!("[DKG] Private Share (hex): {}", hex::encode(private_bytes));

    if let Some(vss) = &valid_share.vss_setup {
        println!("[DKG] Threshold (min signers): {}", vss.min_signers);
        println!("[DKG] Party Indexes: {:?}", vss.I);
    }

    Ok(valid_share)
}

/// 🚀 Helper: Airdrops `lamports` to the given Solana address (Devnet)
pub fn airdrop_funds(address: &str, lamports: u64) -> Result<Pubkey> {
    let rpc_endpoints = [
        "https://api.devnet.solana.com",
        "https://rpc.ankr.com/solana_devnet",
        "https://devnet.rpcpool.com",
        "https://rpc-devnet.helius.xyz",
        "https://devnet-rpc.triton.one",
    ];

    let pubkey = Pubkey::from_str_const(address);
    println!(
        "\n [AIRDROP] Requesting {} lamports for {}\n",
        lamports, address
    );

    let mut success = false;

    // Try multiple RPC endpoints
    for url in rpc_endpoints.iter() {
        let rpc = RpcClient::new(url.to_string());
        println!("[INFO] Trying RPC: {}", url);

        for attempt in 1..=3 {
            println!("[TRY] Attempt {} via {}", attempt, url);

            match rpc.request_airdrop(&pubkey, lamports) {
                Ok(sig) => {
                    println!("[AIRDROP] Tx signature: {}", sig);
                    // Try to confirm
                    match rpc.confirm_transaction(&sig) {
                        Ok(confirmed) if confirmed => {
                            println!("🎉 [SUCCESS] Airdrop confirmed via {}\n", url);
                            success = true;
                            break;
                        }
                        Ok(_) | Err(_) => {
                            println!("[WARN] Confirmation failed. Retrying...");
                            std::thread::sleep(std::time::Duration::from_secs(2));
                        }
                    }
                }
                Err(e) => {
                    println!("[ERROR] Airdrop failed via {}: {:?}", url, e);
                    std::thread::sleep(std::time::Duration::from_secs(2));
                }
            }
        }

        if success {
            break;
        }
    }

    // Check final balance
    let rpc = RpcClient::new("https://api.devnet.solana.com".to_string());
    match rpc.get_balance(&pubkey) {
        Ok(balance) => {
            println!(
                "[BALANCE] {} has {} lamports ({} SOL)\n",
                address,
                balance,
                balance as f64 / 1_000_000_000.0
            );
            if balance < lamports {
                println!("[WARN] Balance lower than requested airdrop ⚠️");
            }
        }
        Err(e) => println!("[WARN] Could not fetch balance: {:?}", e),
    }

    if !success {
        println!("\n [FINAL WARNING] Airdrop failed on all RPCs. You may need to retry later.");
    }

    Ok(pubkey)
}