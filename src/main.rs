use libp2p::{
    SwarmBuilder, identify, identity::Keypair, noise, relay, swarm::{NetworkBehaviour, SwarmEvent}, tcp, yamux
};
use std::error::Error;
use futures::StreamExt;

#[derive(NetworkBehaviour)]
struct RelayBehaviour {
    relay: relay::Behaviour,
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
                relay: relay::Behaviour::new(local_peer_id, relay::Config::default()),
                identify: identify::Behaviour::new(identify::Config::new(
                    "/knot/relay/1.0.0".into(),
                    key.public(),
                )),
            }
        })?
        .build();

    // ESCUCHA EN AMBOS
    swarm.listen_on("/ip4/0.0.0.0/tcp/4001".parse()?)?;             // Puerto TCP
    swarm.listen_on("/ip4/0.0.0.0/udp/4001/quic-v1".parse()?)?;    // Puerto QUIC

    println!("Knot Relay operativo en {}", local_peer_id);

    loop {
        match swarm.select_next_some().await {
            SwarmEvent::NewListenAddr { address, .. } => println!("Escuchando en: {address}"),
            SwarmEvent::Behaviour(RelayBehaviourEvent::Identify(identify::Event::Received { peer_id, info, .. })) => {
                println!("Nodo identificado: {peer_id} | Agente: {}", info.agent_version);
            }
            _ => {}
        }
    }
}