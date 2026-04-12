use libp2p::{
    PeerId, SwarmBuilder, identify, identity::Keypair, kad, noise, ping, relay, swarm::{NetworkBehaviour, SwarmEvent}, tcp, yamux
};
use std::{env, error::Error};
use futures::StreamExt;

#[derive(NetworkBehaviour)]
struct RelayBehaviour {
    relay: relay::Behaviour,
    identify: identify::Behaviour,
    ping: ping::Behaviour,
    kademlia: kad::Behaviour<kad::store::MemoryStore>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    let public_ip = args.get(1).unwrap();

    let local_key = libp2p::identity::Keypair::generate_ed25519();
    let local_peer_id = libp2p::PeerId::from(local_key.public());

    #[cfg(debug_assertions)]
    tracing_subscriber::fmt()
        .with_env_filter("libp2p=debug")
        .init();

    let mut swarm = SwarmBuilder::with_existing_identity(local_key)
        .with_tokio()
        // 1. TRANSPORTE TCP (Necesario para compatibilidad de Relay v2)
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        // 2. TRANSPORTE QUIC (Para el "Buffet de Bytes" de alto rendi
        .with_quic()
        .with_behaviour(|key: &Keypair| {
            let peer_id = PeerId::from(key.public());

            let ping = ping::Behaviour::new(ping::Config::default());

            let mut kademlia = kad::Behaviour::new(
                peer_id,
                kad::store::MemoryStore::new(peer_id),
            );
            kademlia.set_mode(Some(kad::Mode::Server));
            
            RelayBehaviour {
                relay: relay::Behaviour::new(local_peer_id, relay::Config::default()),
                identify: identify::Behaviour::new(identify::Config::new(
                    "/knot/1.0.0".into(),
                    key.public(),
                )),
                ping,
                kademlia
            }
        })?
        .with_swarm_config(|c| c.with_idle_connection_timeout(std::time::Duration::from_secs(60)))
        .build();

    // ESCUCHA EN AMBOS
    swarm.listen_on("/ip4/0.0.0.0/tcp/4001".parse()?)?;             // Puerto TCP
    //swarm.listen_on("/ip4/0.0.0.0/udp/4001/quic-v1".parse()?)?;    // Puerto QUIC

    swarm.add_external_address(format!("/ip4/{}/tcp/4001", public_ip).parse().unwrap());

    println!("Knot Relay operativo en {}", local_peer_id);

    loop {
        match swarm.select_next_some().await {
            SwarmEvent::NewListenAddr { address, .. } => println!("Escuchando en: {address}"),
            SwarmEvent::Behaviour(RelayBehaviourEvent::Identify(identify::Event::Received { peer_id, info, .. })) => {
                println!("Nodo identificado: {peer_id} | Agente: {}", info.agent_version);
            }
            SwarmEvent::Behaviour(RelayBehaviourEvent::Relay(relay::Event::ReservationReqAccepted { src_peer_id, renewed })) => {
                println!("New reservation: {} | renewed: {}", src_peer_id, renewed);
            }
            SwarmEvent::Behaviour(RelayBehaviourEvent::Relay(relay::Event::CircuitReqAccepted { src_peer_id, dst_peer_id })) => {
                println!("Circuit Accepted: {} -> {}", src_peer_id, dst_peer_id);
            }
            SwarmEvent::Behaviour(RelayBehaviourEvent::Relay(relay::Event::ReservationReqDenied { src_peer_id, status: _ })) => {
                println!("Denied reservation: {}", src_peer_id);
            }
            SwarmEvent::Behaviour(RelayBehaviourEvent::Relay(relay::Event::CircuitReqDenied { src_peer_id, dst_peer_id, status })) => {
                println!("Circuit Denied: {} -> {} - {:?}", src_peer_id, dst_peer_id, status);
            }
            _ => {}
        }
    }
}