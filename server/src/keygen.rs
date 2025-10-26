use anyhow::Result;
use dkg_tcp::{TcpIncoming, TcpOutgoing};
use givre::ciphersuite::{AdditionalEntropy};
use givre::generic_ec::{EncodedScalar, NonZero, SecretScalar, curves::Ed25519};
use givre::key_share::DirtyKeyShare;
use givre::keygen::{ExecutionId, ThresholdMsg, keygen};
use givre::keygen::{key_share::Valid, security_level::SecurityLevel128};
use hex;
use rand_core::OsRng;
use round_based::MpcParty;
use sha2::Sha256;
use std::convert::TryInto;
use tokio::net::TcpStream;

type KeygenMsg = ThresholdMsg<Ed25519, SecurityLevel128, Sha256>;

/// Runs the DKG protocol for this participant and returns the generated private share.
pub async fn generate_private_share(
    std_stream_dkg: std::net::TcpStream,
    id: u64,
) -> Result<Valid<DirtyKeyShare<Ed25519>>> {
    // Convert std streams to tokio streams
    let reader_stream_dkg = TcpStream::from_std(std_stream_dkg.try_clone()?)?;
    let writer_stream_dkg = TcpStream::from_std(std_stream_dkg)?;

    let incoming = TcpIncoming::<KeygenMsg>::new(reader_stream_dkg, id);
    let outgoing = TcpOutgoing::<KeygenMsg>::new(writer_stream_dkg, id);

    // Initialize builder for 2-of-2 threshold (adjust as needed)
    let eid = ExecutionId::new(b"session-001");
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
    let encoded: EncodedScalar<Ed25519> =
        <NonZero<SecretScalar<Ed25519>> as AdditionalEntropy<
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
