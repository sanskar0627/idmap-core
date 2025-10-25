use anyhow::Result;
use dkg_tcp::{TcpIncoming, TcpOutgoing};
use givre::ciphersuite::AdditionalEntropy;
use givre::generic_ec::curves::Ed25519;
use givre::generic_ec::{EncodedScalar, NonZero, SecretScalar};
use givre::keygen::security_level::SecurityLevel128;
use givre::keygen::{ExecutionId, ThresholdMsg, keygen};
use givre::signing;
use givre::signing::full_signing::Msg;
use hex;
use rand_core::OsRng;
use round_based::MpcParty;
use sha2::Sha256;
use tokio::net::TcpStream;

type KeygenMsg = ThresholdMsg<Ed25519, SecurityLevel128, Sha256>;
type SigningMsg = Msg<Ed25519>;

#[tokio::main]
async fn main() -> Result<()> {
    let id: u64 = 1;
    let socket = TcpStream::connect("127.0.0.1:7000").await?;
    println!("[CLIENT] Connected to server");

    let std_stream = socket.into_std()?;
    std_stream.set_nonblocking(true)?;
    let std_stream_dkg = std_stream.try_clone()?;
    let std_stream_sign = std_stream.try_clone()?;

    let reader_stream_dkg = tokio::net::TcpStream::from_std(std_stream_dkg.try_clone()?)?;
    let writer_stream_dkg = tokio::net::TcpStream::from_std(std_stream_dkg)?;
    let reader_stream_sign = tokio::net::TcpStream::from_std(std_stream_sign.try_clone()?)?;
    let writer_stream_sign = tokio::net::TcpStream::from_std(std_stream_sign)?;

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

    // ========== SIGNING PHASE ==========
    let incoming = TcpIncoming::<SigningMsg>::new(reader_stream_sign, id);
    let outgoing = TcpOutgoing::<SigningMsg>::new(writer_stream_sign, id);
    let party = MpcParty::connected((incoming, outgoing));

    let i = id as u16;
    let parties_indexes_at_keygen = [0, 1];
    let data_to_sign = b"transaction payload";

    let sig = signing::<givre::ciphersuite::Ed25519>(
        i,
        &valid_shares,
        &parties_indexes_at_keygen,
        data_to_sign,
    )
    .sign(&mut rng, party)
    .await?;

    println!("Signature created successfully! [client]");
    println!("r: {}", hex::encode(sig.r.to_bytes()));
    Ok(())
}
