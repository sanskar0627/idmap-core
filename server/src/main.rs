use dkg_tcp::{keygen, sign};

use anyhow::Result;
use hex;
use tokio::net::TcpListener;

use solana_pubkey::Pubkey;
use solana_rpc_client::rpc_client::RpcClient;
use solana_sdk::signature::Signature;
use solana_transaction::Transaction;
use solana_message::Message;

#[tokio::main]
async fn main() -> Result<()> {
    let id: u64 = 0;

    let listener = TcpListener::bind("0.0.0.0:7000").await?;
    println!("[SERVER] Listening on 0.0.0.0:7000");

    let (socket, addr) = listener.accept().await?;
    println!("[SERVER] Client connected from {:?}", addr);

    // Convert to std
    let std_stream = socket.into_std()?;
    std_stream.set_nonblocking(true)?;

    // Clone for DKG and signing phases
    let std_stream_dkg = std_stream.try_clone()?;
    let std_stream_sign = std_stream.try_clone()?;
    let std_stream_send = std_stream.try_clone()?;

    // ================= DKG PHASE ================= [should be called only once per user]
    let session = b"session-001"; // make it dynamic
    let valid_shares = keygen::generate_private_share(std_stream_dkg, id, session).await?;

    let pubkey_bytes = valid_shares.shared_public_key().to_bytes(true);
    let solana_address = bs58::encode(pubkey_bytes).into_string();
    println!("SOLANA ADDRESS: {}", solana_address);
    println!(
        "shared public key (hex): {}",
        hex::encode(valid_shares.shared_public_key().to_bytes(true))
    );
    println!("finished DKG for server");

    // airdrop some sol after generating keys for testing
    let pubkey = keygen::airdrop_funds(&solana_address, 5_000_000)?;
    std::thread::sleep(std::time::Duration::from_secs(60));

    // ================= SIGNING PHASE =================
    let lamports = 1_000_000;
    let to = Pubkey::from_str_const("3fHQhTVCHerqk69GwqXhWbG4zRArCUqi6Bhnu7pTm5mj"); // get this from the user

    // create the message to sign
    let message: Message =
        sign::create_transfer_message(&solana_address, &to.to_string(), lamports)
            .expect("error generating message to be signed");

    // send to the other server from here
    let _ = sign::send_message_to_other_server(id, std_stream_send, message.clone()).await?;

    // start signing protocol
    let (r_slice, z_slice) =
        sign::run_signing_phase(id, valid_shares, std_stream_sign, message.serialize()).await?;

    // ================= BROADCAST TO THE SOLANA BLOCKCHAIN =================

    println!("broadcasting to the solana");
    let rpc = RpcClient::new("https://api.devnet.solana.com".to_string());

    // for testing
    let balance = rpc.get_balance(&pubkey)?;
    println!("[SERVER] Balance after airdrop: {}", balance);
    if balance < 1_000_000 {
        panic!("Not enough funds to send transaction!");
    }

    let mut tx = Transaction::new_unsigned(message);
    let sig_bytes = [r_slice, z_slice].concat();
    let sol_sig = Signature::try_from(sig_bytes.clone())
        .expect("err creating solana signature from dkg signature");

    tx.signatures = vec![sol_sig];
    let tx_sig = rpc.send_transaction(&tx)?;
    println!("Broadcasted tx: {}", tx_sig);

    Ok(())
}
