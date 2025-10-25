use std::convert::TryInto;
use tokio::net::TcpStream;

use anyhow::Result;
use dkg_tcp::{TcpIncoming, TcpOutgoing};
use futures::StreamExt;
use givre::ciphersuite::AdditionalEntropy; // for private share to_bytes
use givre::ciphersuite::NormalizedPoint;
use givre::generic_ec::Point;
use givre::generic_ec::Scalar;
use givre::generic_ec::curves::Ed25519;
use givre::generic_ec::{EncodedScalar, NonZero, SecretScalar};
use givre::key_share::DirtyKeyShare;
use givre::keygen::key_share::Valid;
use givre::keygen::security_level::SecurityLevel128;
use givre::keygen::{ExecutionId, ThresholdMsg, keygen};
use givre::signing;
use givre::signing::{aggregate::aggregate, full_signing::Msg};
use rand_core::OsRng;
use round_based::MpcParty;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use tokio::net::TcpListener;

use hex;

type KeygenMsg = ThresholdMsg<Ed25519, SecurityLevel128, Sha256>;
type SigningMsg = Msg<Ed25519>;

#[derive(Serialize, Deserialize, Debug)]
pub struct SignatureMsg {
    pub signer_index: u16,
    pub r: Vec<u8>,
    pub z: Vec<u8>,
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
    let std_stream_dkg = std_stream.try_clone()?;
    let std_stream_sign = std_stream.try_clone()?;
    let std_stream_receive = std_stream.try_clone()?;

    // Convert clones to tokio streams
    let reader_stream_dkg = tokio::net::TcpStream::from_std(std_stream_dkg.try_clone()?)?;
    let writer_stream_dkg = tokio::net::TcpStream::from_std(std_stream_dkg)?;

    let reader_stream_sign = tokio::net::TcpStream::from_std(std_stream_sign.try_clone()?)?;
    let writer_stream_sign = tokio::net::TcpStream::from_std(std_stream_sign)?;

    let reader_stream_receiver = TcpStream::from_std(std_stream_receive)?;

    // ================= DKG PHASE =================
    let incoming = TcpIncoming::<KeygenMsg>::new(reader_stream_dkg, id);
    let outgoing = TcpOutgoing::<KeygenMsg>::new(writer_stream_dkg, id);

    let eid = ExecutionId::new(b"session-001");
    let builder = keygen::<Ed25519>(eid, id as u16, 2).set_threshold(2);
    let mut rng = OsRng;

    let party = MpcParty::connected((incoming, outgoing));
    println!("starting DKG for server");

    let valid_shares: Valid<DirtyKeyShare<Ed25519>>;

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
    let key_share = valid_shares;
    let data_to_sign = b"transaction payload";

    let _signature = signing::<givre::ciphersuite::Ed25519>(
        i,
        &key_share,
        &parties_indexes_at_keygen,
        data_to_sign,
    )
    .sign(&mut rng, party)
    .await?;

    println!("partial signature created successfully! [server]");
    println!("r: {}", hex::encode(_signature.r.to_bytes()));
    let z_bytes_generic: <givre::ciphersuite::Ed25519 as givre::ciphersuite::Ciphersuite>::ScalarBytes =
        <Scalar<Ed25519> as AdditionalEntropy<givre::ciphersuite::Ed25519>>::to_bytes(&_signature.z);

    let z_bytes: [u8; 32] = z_bytes_generic
        .as_ref()
        .try_into()
        .expect("must be 32 bytes");
    println!("z: {}", hex::encode(z_bytes));

    // ================= RECEIVE CLIENT PARTIAL SIGNATURE =================
    let mut incoming_sig = TcpIncoming::<SignatureMsg>::new(reader_stream_receiver, id);

    // Await the next incoming message
    // Await the next incoming message
    if let Some(Ok(incoming_msg)) = incoming_sig.next().await {
        let sig_msg: SignatureMsg = incoming_msg.msg;
        println!("[SERVER] Received signature from client: {:?}", sig_msg);

        // Convert r bytes into Point
        let r_point = Point::<Ed25519>::from_bytes(&sig_msg.r)
            .map_err(|e| anyhow::anyhow!("Invalid r point: {:?}", e))?;

        // Normalize the point (returns Result<NormalizedPoint, _>)
        let r_normalized =
            NormalizedPoint::<givre::ciphersuite::Ed25519, _>::try_normalize(r_point)
                .map_err(|_| anyhow::anyhow!("r point is not normalized"))?;

        // TODO: resolve the error: [InvalidScalar] for 'z', runtime error
        // Ensure exactly 32 bytes
        let z_bytes: [u8; 32] = sig_msg.z.as_slice().try_into().expect("z must be 32 bytes");

        // Convert back into Scalar<Ed25519>
        let z_scalar = Scalar::<Ed25519>::from_be_bytes(&z_bytes)
            .map_err(|e| anyhow::anyhow!("Invalid z scalar: {:?}", e))?;

        // Construct a partial signature
        let client_partial_sig =
            givre::signing::aggregate::Signature::<givre::ciphersuite::Ed25519> {
                r: r_normalized,
                z: z_scalar,
            };

        // Now you can aggregate it with your own partial signature
        println!("[SERVER] Constructed client partial signature successfully");
    }

    Ok(())
}
