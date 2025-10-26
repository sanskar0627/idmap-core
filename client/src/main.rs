use dkg_tcp::{keygen, sign, TcpIncoming};

use anyhow::Result;
use hex;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MessageToSign {
    pub data: Vec<u8>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let id: u64 = 1;
    let socket = TcpStream::connect("127.0.0.1:7000").await?;
    println!("[CLIENT] Connected to server");

    let std_stream = socket.into_std()?;
    std_stream.set_nonblocking(true)?;
    let std_stream_dkg = std_stream.try_clone()?;
    let std_stream_sign = std_stream.try_clone()?;
    let std_stream_message = std_stream.try_clone()?;

    let reader_stream_message = TcpStream::from_std(std_stream_message.try_clone()?)?;

    // ========== DKG PHASE ==========
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
    std::thread::sleep(std::time::Duration::from_secs(60));

    // ========== SIGNING PHASE ==========
    let mut incoming_message = TcpIncoming::<MessageToSign>::new(reader_stream_message, id);
    let mut data_to_sign: Option<Vec<u8>> = None;

    // get the message to sign from the other server
    while let Some(result) = incoming_message.next().await {
        match result {
            Ok(msg) => {
                println!("Received {} bytes to sign", msg.msg.data.len());
                data_to_sign = Some(msg.msg.data); // store it
                break; // exit after receiving one message
            }
            Err(e) => {
                eprintln!("Error receiving message: {:?}", e);
                break; // stop trying if error
            }
        }
    }

    let data_to_sign = match data_to_sign {
        Some(data) => data,
        None => {
            eprintln!("No data received to sign!");
            return Ok(());
        }
    };

    // start signing protocol
    let (r_slice, z_slice) =
        sign::run_signing_phase(id, valid_shares, std_stream_sign, data_to_sign).await?;

    println!("Signature created successfully! [client]");
    println!("r: {}", hex::encode(r_slice));
    println!("z: {}", hex::encode(z_slice));

    Ok(())
}
