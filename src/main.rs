use libp2p::{
    SwarmBuilder, identify, identity::Keypair, noise, swarm::{NetworkBehaviour, SwarmEvent}, tcp, yamux
};
use std::error::Error;
use futures::StreamExt;

#[derive(NetworkBehaviour)]
struct RelayBehaviour {
    relay: libp2p_relay::Behaviour,
    identify: identify::Behaviour,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let local_key = libp2p::identity::Keypair::generate_ed25519();
    let local_peer_id = libp2p::PeerId::from(local_key.public());

    let mut swarm = SwarmBuilder::with_existing_identity(local_key)
        .with_tokio()
        // 1. TRANSPORTE TCP (Necesario para compatibilidad de Relay v2)
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        // 2. TRANSPORTE QUIC (Para el "Buffet de Bytes" de alto rendimiento)
        .with_quic()
        .with_behaviour(|key: &Keypair| {
            RelayBehaviour {
                relay: libp2p_relay::Behaviour::new(local_peer_id, libp2p_relay::Config::default()),
                identify: identify::Behaviour::new(identify::Config::new(
                    "/knot/relay/1.0.0".into(),
                    key.public(),
                )),
            }
        })?
        .with_swarm_config(|c| c.with_idle_connection_timeout(std::time::Duration::from_secs(60)))
        .build();

    // ESCUCHA EN AMBOS
    swarm.listen_on("/ip4/0.0.0.0/tcp/4001".parse()?)?;             // Puerto TCP
    //swarm.listen_on("/ip4/0.0.0.0/udp/4001/quic-v1".parse()?)?;    // Puerto QUIC

    println!("Knot Relay operativo en {}", local_peer_id);

    loop {
        match swarm.select_next_some().await {
            SwarmEvent::NewListenAddr { address, .. } => println!("Escuchando en: {address}"),
            SwarmEvent::Behaviour(RelayBehaviourEvent::Identify(identify::Event::Received { peer_id, info, .. })) => {
                println!("Nodo identificado: {peer_id} | Agente: {}", info.agent_version);
            }
            SwarmEvent::Behaviour(RelayBehaviourEvent::Relay(libp2p_relay::Event::ReservationReqAccepted { src_peer_id, renewed })) => {
                println!("New reservation: {} | renewed: {}", src_peer_id, renewed);
            }
            SwarmEvent::Behaviour(RelayBehaviourEvent::Relay(libp2p_relay::Event::CircuitReqAccepted { src_peer_id, dst_peer_id })) => {
                println!("Circuit Accepted: {} -> {}", src_peer_id, dst_peer_id);
            }
            SwarmEvent::Behaviour(RelayBehaviourEvent::Relay(libp2p_relay::Event::ReservationReqDenied { src_peer_id, status: _ })) => {
                println!("Denied reservation: {}", src_peer_id);
            }
            SwarmEvent::Behaviour(RelayBehaviourEvent::Relay(libp2p_relay::Event::CircuitReqDenied { src_peer_id, dst_peer_id, status: _ })) => {
                println!("Circuit Denied: {} -> {}", src_peer_id, dst_peer_id);
            }
            _ => {}
        }
    }
}