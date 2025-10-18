use anyhow::Result;
use dkg_tcp::{TcpIncoming, TcpOutgoing};
use givre::generic_ec::curves::Ed25519;
use givre::keygen::security_level::SecurityLevel128;
use givre::keygen::{ExecutionId, ThresholdMsg, keygen};
use rand_core::OsRng;
use round_based::MpcParty;
use sha2::Sha256;
use tokio::net::TcpStream;

type Msg = ThresholdMsg<Ed25519, SecurityLevel128, Sha256>;

#[tokio::main]
async fn main() -> Result<()> {
    let id: u64 = 1;
    let socket = TcpStream::connect("127.0.0.1:7000").await?;
    println!("[CLIENT] Connected to server");

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
    let builder = keygen::<Ed25519>(eid, id as u16, 2).set_threshold(1);
    let mut rng = OsRng;

    let party = MpcParty::connected((incoming, outgoing));
    println!("starting dkg for client");
    let valid_share = builder.start(&mut rng, party).await?;
    println!("finished dkg for client");
    

    println!(
        "[CLIENT] DKG completed ✅ Shared Public Key: {:?}",
        valid_share.shared_public_key()
    );

    Ok(())
}
