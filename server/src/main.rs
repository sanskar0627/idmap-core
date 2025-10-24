use anyhow::Result;
use dkg_tcp::{TcpIncoming, TcpOutgoing};
use givre::ciphersuite::AdditionalEntropy; // for private share to_bytes
use givre::generic_ec::{SecretScalar, EncodedScalar, NonZero};
use givre::generic_ec::curves::Ed25519;
use givre::keygen::security_level::SecurityLevel128;
use givre::keygen::{ExecutionId, ThresholdMsg, keygen};
use rand_core::OsRng;
use round_based::MpcParty;
use sha2::Sha256;
use tokio::net::TcpListener;

use hex;

type Msg = ThresholdMsg<Ed25519, SecurityLevel128, Sha256>;

#[tokio::main]
async fn main() -> Result<()> {
    let id: u64 = 0;
    let listener = TcpListener::bind("0.0.0.0:7000").await?;
    println!("[SERVER] Listening on 0.0.0.0:7000");

    let (socket, addr) = listener.accept().await?;
    println!("[SERVER] Client connected from {:?}", addr);

    // Convert to std and clone
    let std_stream = socket.into_std()?;
    std_stream.set_nonblocking(true)?;
    let std_stream_clone = std_stream.try_clone()?;

    // Convert back to tokio
    let reader_stream = tokio::net::TcpStream::from_std(std_stream_clone)?;
    let writer_stream = tokio::net::TcpStream::from_std(std_stream)?;

    // Create transports
    let incoming = TcpIncoming::<Msg>::new(reader_stream, id);
    let outgoing = TcpOutgoing::<Msg>::new(writer_stream, id);

    // Setup DKG
    let eid = ExecutionId::new(b"session-001");
    let builder = keygen::<Ed25519>(eid, id as u16, 2).set_threshold(2);
    let mut rng = OsRng;

    let party = MpcParty::connected((incoming, outgoing));
    println!("starting dkg for server");

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

            // private shares
            let private_share = &valid_share.x;
            let encoded: EncodedScalar<Ed25519> =
                <NonZero<SecretScalar<Ed25519>> as AdditionalEntropy<givre::ciphersuite::Ed25519>>::to_bytes(
                    private_share,
                );
            let private_bytes: [u8; 32] = encoded
                .as_ref()
                .try_into()
                .expect("EncodedScalar must be 32 bytes");
            println!("Private Share (hex): {}", hex::encode(private_bytes));

            if let Some(vss) = &valid_share.vss_setup {
                println!("Threshold (min signers): {}", vss.min_signers);
                println!("Party Indexes: {:?}", vss.I);
            }
        }
        Err(e) => {
            eprintln!("DKG failed with error: {:#?}", e);
            return Err(e.into());
        }
    }

    println!("finished dkg for server");
    Ok(())
}
