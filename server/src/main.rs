mod keygen;
mod sign;

use anyhow::Result;
use futures::SinkExt;
use std::convert::TryInto;
use std::fmt::Debug;
use tokio::net::{TcpListener, TcpStream};

use dkg_tcp::{TcpIncoming, TcpOutgoing};
use givre::ciphersuite::{AdditionalEntropy, Ciphersuite, Ed25519 as CsEd25519};
use givre::generic_ec::{EncodedScalar, NonZero, Scalar, SecretScalar, curves::Ed25519};
use givre::key_share::DirtyKeyShare;
use givre::keygen::{ExecutionId, ThresholdMsg, keygen};
use givre::keygen::{key_share::Valid, security_level::SecurityLevel128};
use givre::signing;
use givre::signing::{aggregate::Signature, full_signing::Msg};
use round_based::{MpcParty, Outgoing};

use hex;
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use solana_instruction::Instruction;
use solana_message::Message;
use solana_program::instruction::AccountMeta;
use solana_pubkey::Pubkey;
use solana_rpc_client::rpc_client::RpcClient;
use solana_sdk::signature::Signature as SolSignature;
use solana_transaction::Transaction;

type KeygenMsg = ThresholdMsg<Ed25519, SecurityLevel128, Sha256>;
type SigningMsg = Msg<Ed25519>;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MessageToSign {
    pub data: Vec<u8>,
}

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
    let std_stream_dkg: std::net::TcpStream = std_stream.try_clone()?;
    let std_stream_sign = std_stream.try_clone()?;
    let std_stream_send = std_stream.try_clone()?;

    // Convert clones to tokio streams
    let reader_stream_dkg: TcpStream = TcpStream::from_std(std_stream_dkg.try_clone()?)?;
    let writer_stream_dkg = TcpStream::from_std(std_stream_dkg)?;
    let reader_stream_sign = TcpStream::from_std(std_stream_sign.try_clone()?)?;
    let writer_stream_sign = TcpStream::from_std(std_stream_sign)?;
    let writer_stream_send = TcpStream::from_std(std_stream_send)?;

    // ================= DKG PHASE =================
    let incoming = TcpIncoming::<KeygenMsg>::new(reader_stream_dkg, id);
    let outgoing = TcpOutgoing::<KeygenMsg>::new(writer_stream_dkg, id);
    let mut outgoing_send = TcpOutgoing::<MessageToSign>::new(writer_stream_send, id);

    let eid = ExecutionId::new(b"session-001"); // need to be synamic for each session
    let builder = keygen::<Ed25519>(eid, id as u16, 2).set_threshold(2);
    let mut rng = OsRng;

    let party = MpcParty::connected((incoming, outgoing));
    println!("starting DKG for server");

    let valid_shares = keygen::generate_private_share(std_stream_dkg, id).await?;

    match builder.start(&mut rng, party).await {
        Ok(valid_share) => {
            println!("DKG completed for server");

            let shared_public_key = valid_share.shared_public_key();
            println!(
                "Shared Public Key (hex): {}",
                hex::encode(shared_public_key.to_bytes(true))
            );

            let party_index = valid_share.i;
            println!("Party Index: {}", party_index);

            let public_shares = &valid_share.public_shares;

            for (idx, pk) in public_shares.iter().enumerate() {
                println!(
                    "Public Share {} (hex): {}",
                    idx,
                    hex::encode(pk.to_bytes(true))
                );
            }

            // Private shares
            let private_share = &valid_share.x;
            let encoded: EncodedScalar<Ed25519> =
                <NonZero<SecretScalar<Ed25519>> as AdditionalEntropy<
                    givre::ciphersuite::Ed25519,
                >>::to_bytes(private_share);
            let private_bytes: [u8; 32] = encoded
                .as_ref()
                .try_into()
                .expect("EncodedScalar must be 32 bytes");
            println!("Private Share (hex): {}", hex::encode(private_bytes));

            if let Some(vss) = &valid_share.vss_setup {
                println!("Threshold (min signers): {}", vss.min_signers);
                println!("Party Indexes: {:?}", vss.I);
            }

            valid_shares = valid_share;
        }
        Err(e) => {
            eprintln!("DKG failed with error: {:#?}", e);
            return Err(e.into());
        }
    }

    let pubkey_bytes = valid_shares.shared_public_key().to_bytes(true);
    let solana_address = bs58::encode(pubkey_bytes).into_string();
    println!("SOLANA ADDRESS: {}", solana_address);

    println!(
        "shared public key (hex): {}",
        hex::encode(valid_shares.shared_public_key().to_bytes(true))
    );
    println!("finished DKG for server");

    // ================= SIGNING PHASE =================
    let incoming = TcpIncoming::<SigningMsg>::new(reader_stream_sign, id);
    let outgoing = TcpOutgoing::<SigningMsg>::new(writer_stream_sign, id);
    let party = MpcParty::connected((incoming, outgoing));

    let i = id as u16; // signer index
    let parties_indexes_at_keygen: [u16; 2] = [0, 1];
    let key_share: Valid<DirtyKeyShare<Ed25519>> = valid_shares;

    // message to sign
    println!("creating message to sign");
    let _rpc = RpcClient::new("https://api.devnet.solana.com".to_string());
    let from = Pubkey::from_str_const(&solana_address);
    let to = Pubkey::from_str_const("3fHQhTVCHerqk69GwqXhWbG4zRArCUqi6Bhnu7pTm5mj");
    let lamports: u64 = 1_000_000;
    const SYSTEM_PROGRAM_ID: &str = "11111111111111111111111111111111";

    // airdrop
    let sig = _rpc.request_airdrop(&from, 500_000_00)?; // 0.5 SOL = 500_000_000 lamports
    println!("Airdrop transaction signature: {}", sig);

    _rpc.confirm_transaction(&sig)?;
    println!("Airdrop confirmed! Balance ready for use.");

    // 8 bytes: first 4 = transfer discriminator, next 8 = lamports
    // The transfer discriminator for system program = 2
    let mut data = vec![];
    data.extend_from_slice(&2u32.to_le_bytes()); // "transfer" enum variant index
    data.extend_from_slice(&lamports.to_le_bytes()); // amount

    let instruction = Instruction {
        program_id: Pubkey::from_str_const(SYSTEM_PROGRAM_ID),
        accounts: vec![AccountMeta::new(from, true), AccountMeta::new(to, false)],
        data,
    };

    // send message to the other server to sign the same message
    let mut message: Message = Message::new(&[instruction], Some(&from));
    message.recent_blockhash = _rpc.get_latest_blockhash()?;
    let message_data: Vec<u8> = message.serialize();

    let signing_msg: MessageToSign = MessageToSign {
        data: message_data.clone(),
    };
    let outgoing_msg: Outgoing<MessageToSign> = Outgoing::p2p(1, signing_msg.clone());

    // send message_data to the other server
    outgoing_send.send(outgoing_msg).await?;

    let _signature: Signature<CsEd25519> =
        signing::<CsEd25519>(i, &key_share, &parties_indexes_at_keygen, &message_data)
            .sign(&mut rng, party) // sign gives the full signature, and uses aggregate internally
            .await?;

    println!("signature created successfully! [server]");
    println!("r: {}", hex::encode(_signature.r.to_bytes()));
    let z_bytes_generic: <CsEd25519 as Ciphersuite>::ScalarBytes =
        <Scalar<Ed25519> as AdditionalEntropy<CsEd25519>>::to_bytes(&_signature.z);

    let z_bytes: [u8; 32] = z_bytes_generic
        .as_ref()
        .try_into()
        .expect("must be 32 bytes");

    let r_bytes_sig = _signature.r.to_bytes();
    let r_slice = r_bytes_sig.as_ref();
    let z_bytes_sig = z_bytes.clone();
    let z_slice: &[u8] = &z_bytes_sig;
    println!("z: {}", hex::encode(z_bytes));

    // ================= BROADCAST TO THE SOLANA BLOCKCHAIN =================

    let mut tx = Transaction::new_unsigned(message);
    let sig_bytes = [r_slice, z_slice].concat();
    let sol_sig = SolSignature::try_from(sig_bytes.clone())
        .expect("err creating solana signature from dkg signature");

    println!("txn created");
    tx.signatures = vec![sol_sig];
    println!("sig added to sig");

    let tx_sig = _rpc.send_transaction(&tx)?;
    println!("Broadcasted tx: {}", tx_sig);

    Ok(())
}
