// full valid code of server [running]
/*
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
    
    // IF NEED TO GET THE SIGNATURE FROM THE CLIENT [SERVER-2]
    
    // 1. CLIENT-SIDE CODE
    
    
    // --- Send signature to server ---
    
    // serialize the signature
    let mut serialized = vec![0u8; Signature::<CsEd25519>::serialized_len()];
    sig.write_to_slice(&mut serialized);

    let sig_msg = SignatureMsg {
        signer_index: i,
        sig: serialized,
    };

    // Reuse your transport channel to send
    let mut outgoing_sig = TcpOutgoing::<SignatureMsg>::new(writer_stream_send, id);

    // Properly send using SinkExt::send() — async and flushed
    outgoing_sig
        .send(Outgoing {
            recipient: round_based::MessageDestination::OneParty(0), // server is id=0
            msg: sig_msg,
        })
        .await?;

    println!("[CLIENT] Sent signature share to server.");
    
    
    // 2. SERVER-SIDE CODE
    
    
    // ================= RECEIVE CLIENT PARTIAL SIGNATURE =================
    
    let listener = TcpListener::bind("0.0.0.0:7000").await?;
    println!("[SERVER] Listening on 0.0.0.0:7000");

    let (socket, addr) = listener.accept().await?;
    println!("[SERVER] Client connected from {:?}", addr);

    // Convert to std
    let std_stream = socket.into_std()?;
    std_stream.set_nonblocking(true)?;
    
    let std_stream_receive = std_stream.try_clone()?;
    let reader_stream_receiver = TcpStream::from_std(std_stream_receive)?;
    
    
    let mut incoming_sig = TcpIncoming::<SignatureMsg>::new(reader_stream_receiver, id);

    // Await the next incoming message
    if let Some(Ok(incoming_msg)) = incoming_sig.next().await {
        let sig_msg: SignatureMsg = incoming_msg.msg;
        println!("[SERVER] Received signature from client: {:?}", sig_msg);

        let sender = sig_msg.signer_index;
        let client_sig_serialized = sig_msg.sig;

        let deserialized = Signature::<CsEd25519>::read_from_slice(&client_sig_serialized)
            .expect("invalid signature bytes");

        println!("sender: {}", sender);
        println!("r: {}", hex::encode(deserialized.r.to_bytes()));
        let z_bytes_generic: <CsEd25519 as Ciphersuite>::ScalarBytes =
            <Scalar<Ed25519> as AdditionalEntropy<CsEd25519>>::to_bytes(&deserialized.z);

        let z_bytes: [u8; 32] = z_bytes_generic
            .as_ref()
            .try_into()
            .expect("must be 32 bytes");
        println!("z: {}", hex::encode(z_bytes));
        println!("[SERVER] Constructed client partial signature successfully");


    // 3. FOR AGGREGATION [NOT NEEDED IF USING 'sign', i.e. full-signing]
    
        // ================= AGGREGATE PARTIAL SIGNATURE =================

        let sig_server: Signature<CsEd25519> = _signature; // your own partial signature
        let sig_client: Signature<CsEd25519> = deserialized;

        // key_info
        let key_info: &Valid<DirtyKeyInfo<Ed25519>> = key_share.as_ref();

        // publicCommitments
        let hiding_comm_bytes = sig_server.r.to_bytes(); // returns [u8; 32] usually
        let hiding_comm: Point<Ed25519> = Point::from_bytes(&hiding_comm_bytes)
            .expect("Failed to convert normalized point to Point<Ed25519>");

        let binding_comm_bytes = sig_client.r.to_bytes();
        let binding_comm: Point<Ed25519> = Point::from_bytes(&binding_comm_bytes)
            .expect("Failed to convert normalized point to Point<Ed25519>");

        let public_commitments: PublicCommitments<Ed25519> = PublicCommitments {
            hiding_comm,
            binding_comm,
        };

        // sigShare
        let sig_share_server: SigShare<Ed25519> = SigShare(sig_server.z);
        let sig_share_client: SigShare<Ed25519> = SigShare(sig_client.z);

        // message
        let message = data_to_sign;

        // signer tuples: (signer_index, public_commitments, sig_share)
        let signers_list: &[(u16, PublicCommitments<Ed25519>, SigShare<Ed25519>); 2] = &[
            (id as u16, public_commitments.clone(), sig_share_server),
            (sender, public_commitments.clone(), sig_share_client),
        ];

        for (idx, (signer_index, comm, sig_share)) in signers_list.iter().enumerate() {
            println!(
                "Signer {} -> index: {}, r: {:?}, z: {:?}",
                idx, signer_index, comm, sig_share.0
            );
        }

        println!("--------------------------AGGREGATE CALLED-------------------------------------");
        // aggregate function call
        // let full_sig = aggregate::<CsEd25519>(key_info, signers_list, message);
        match aggregate::<CsEd25519>(key_info, signers_list, message) {
            Ok(sig) => {
                println!("final signature value");
                println!("r: {}", hex::encode(sig.r.to_bytes()));

                let z_bytes_generic: <CsEd25519 as Ciphersuite>::ScalarBytes =
                    <Scalar<Ed25519> as AdditionalEntropy<CsEd25519>>::to_bytes(&sig.z);

                let z_bytes: [u8; 32] = z_bytes_generic
                    .as_ref()
                    .try_into()
                    .expect("must be 32 bytes");
                println!("z: {}", hex::encode(z_bytes));
            }
            Err(e) => {
                eprintln!("Failed to aggregate signature: {:?}", e);
                eprintln!("{}", e.to_string());
            }
        }
    }
*/
