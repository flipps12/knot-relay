use libp2p::{
    identify,
    relay,
    swarm::{NetworkBehaviour, SwarmEvent},
    Multiaddr,
};
use std::error::Error;
use std::time::Duration;
use futures::StreamExt;

// 1. IMPORTANTE: El derive genera automáticamente el enum de eventos
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "OutEvent")] // Definimos un nombre claro para el enum de salida
struct RelayBehaviour {
    relay: relay::Behaviour,
    identify: identify::Behaviour,
}

// 2. Definimos el Enum de eventos que el derive usará
enum OutEvent {
    Relay(relay::Event),
    Identify(identify::Event),
}

// Implementación manual de la conversión para que el derive sepa qué hacer
impl From<relay::Event> for OutEvent {
    fn from(event: relay::Event) -> Self {
        OutEvent::Relay(event)
    }
}

impl From<identify::Event> for OutEvent {
    fn from(event: identify::Event) -> Self {
        OutEvent::Identify(event)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let local_key = libp2p::identity::Keypair::generate_ed25519();
    let local_peer_id = libp2p::PeerId::from(local_key.public());
    println!("Knot Relay Server ID: {}", local_peer_id);

    let mut swarm = libp2p::SwarmBuilder::with_existing_identity(local_key)
        .with_tokio()
        .with_quic() 
        .with_behaviour(|key| {
            let local_peer_id = libp2p::PeerId::from(key.public()); // Obtenemos el ID desde la llave
        
            RelayBehaviour {
                // CAMBIO AQUÍ: Usamos .new() con el PeerId y configuración por defecto
                relay: relay::Behaviour::new(
                    local_peer_id, 
                    relay::Config::default()
                ),
                identify: identify::Behaviour::new(identify::Config::new(
                    "/knot/1.0.0".to_string(),
                    key.public(),
                )),
            }
        })?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build();

    let listen_addr: Multiaddr = "/ip4/0.0.0.0/udp/4001/quic-v1".parse()?;
    swarm.listen_on(listen_addr)?;

    loop {
        match swarm.select_next_some().await {
            SwarmEvent::NewListenAddr { address, .. } => {
                println!("Dirección local: {}", address);
            }
            // 3. Usamos el nombre del Enum que definimos arriba
            SwarmEvent::Behaviour(OutEvent::Relay(e)) => {
                println!("Evento de Relay: {:?}", e);
            }
            SwarmEvent::Behaviour(OutEvent::Identify(e)) => {
                // 4. Añadimos el .. para ignorar los campos nuevos como connection_id
                if let identify::Event::Received { peer_id, info, .. } = e {
                    println!("Identificado Peer: {} con Addrs: {:?}", peer_id, info.listen_addrs);
                }
            }
            _ => {}
        }
    }
}