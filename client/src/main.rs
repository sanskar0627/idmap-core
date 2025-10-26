use anyhow::Result;
use dkg_tcp::{TcpIncoming, TcpOutgoing};
use futures::{Sink, SinkExt, StreamExt};
use givre::ciphersuite::AdditionalEntropy;
use givre::ciphersuite::Ed25519 as CsEd25519;
use givre::generic_ec::Scalar;
use givre::generic_ec::curves::Ed25519;
use givre::keygen::security_level::SecurityLevel128;
use givre::keygen::{ExecutionId, ThresholdMsg, keygen};
use givre::signing;
use givre::signing::aggregate::Signature;
use givre::signing::full_signing::Msg;
use hex;
use rand_core::OsRng;
use round_based::MpcParty;
use round_based::Outgoing;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use tokio::net::TcpStream;

use solana_sdk::signature::Signature as SolSignature;

use borsh::{BorshDeserialize, BorshSerialize};
use solana_instruction::Instruction;
use solana_keypair::Keypair;
use solana_message::Message;
use solana_program::instruction::AccountMeta;
use solana_pubkey::Pubkey;
use solana_rpc_client::rpc_client::RpcClient;
use solana_sdk::config::program;
use solana_signer::Signer;
use solana_system_program::system_instruction;
use solana_transaction::Transaction;

type KeygenMsg = ThresholdMsg<Ed25519, SecurityLevel128, Sha256>;
type SigningMsg = Msg<Ed25519>;

#[derive(Serialize, Deserialize, Debug)]
pub struct SignatureMsg {
    pub signer_index: u16,
    pub sig: Vec<u8>,
}

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

    let reader_stream_dkg = TcpStream::from_std(std_stream_dkg.try_clone()?)?;
    let writer_stream_dkg = TcpStream::from_std(std_stream_dkg)?;
    let reader_stream_sign = TcpStream::from_std(std_stream_sign.try_clone()?)?;
    let writer_stream_sign = TcpStream::from_std(std_stream_sign)?;
    let reader_stream_message = TcpStream::from_std(std_stream_message.try_clone()?)?;

    // ========== DKG PHASE ==========
    let incoming = TcpIncoming::<KeygenMsg>::new(reader_stream_dkg, id);
    let outgoing = TcpOutgoing::<KeygenMsg>::new(writer_stream_dkg, id);
    let eid = ExecutionId::new(b"session-001");
    let builder = keygen::<Ed25519>(eid, id as u16, 2).set_threshold(2);
    let mut rng = OsRng;
    let party = MpcParty::connected((incoming, outgoing));

    println!("Starting DKG for client...");
    let valid_shares = builder.start(&mut rng, party).await?;
    println!(" DKG completed for client.");

    println!(
        "Shared Public Key: {}",
        hex::encode(valid_shares.shared_public_key().to_bytes(true))
    );

    let pubkey_bytes = valid_shares.shared_public_key().to_bytes(true);
    let solana_address = bs58::encode(pubkey_bytes).into_string();

    // ========== SIGNING PHASE ==========
    let incoming = TcpIncoming::<SigningMsg>::new(reader_stream_sign, id);
    let outgoing = TcpOutgoing::<SigningMsg>::new(writer_stream_sign, id);
    let mut incoming_message = TcpIncoming::<MessageToSign>::new(reader_stream_message, id);
    let party = MpcParty::connected((incoming, outgoing));

    let i = id as u16;
    let parties_indexes_at_keygen = [0, 1];

    // message to sign
    let _rpc = RpcClient::new("https://api.devnet.solana.com".to_string());
    let from = Pubkey::from_str_const(&solana_address);
    let to = Pubkey::from_str_const("3fHQhTVCHerqk69GwqXhWbG4zRArCUqi6Bhnu7pTm5mj");
    let lamports: u64 = 1_000_000;
    const SYSTEM_PROGRAM_ID: &str = "11111111111111111111111111111111";

    // 8 bytes: first 4 = transfer discriminator, next 8 = lamports
    // The transfer discriminator for system program = 2
    let mut data = vec![];
    data.extend_from_slice(&2u32.to_le_bytes()); // "transfer" enum variant index
    data.extend_from_slice(&lamports.to_le_bytes()); // amount

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

    let sig: signing::aggregate::Signature<givre::ciphersuite::Ed25519> =
        signing::<givre::ciphersuite::Ed25519>(
            i,
            &valid_shares,
            &parties_indexes_at_keygen,
            &data_to_sign,
        )
        .sign(&mut rng, party)
        .await?;

    println!("Signature created successfully! [client]");
    println!("r: {}", hex::encode(sig.r.to_bytes()));

    let z_bytes_generic: <givre::ciphersuite::Ed25519 as givre::ciphersuite::Ciphersuite>::ScalarBytes =
        <Scalar<Ed25519> as AdditionalEntropy<givre::ciphersuite::Ed25519>>::to_bytes(&sig.z);

    let z_bytes: [u8; 32] = z_bytes_generic
        .as_ref()
        .try_into()
        .expect("must be 32 bytes");

    println!("z: {}", hex::encode(z_bytes));

    Ok(())
}
