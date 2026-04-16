use libp2p::{
    Multiaddr,
    PeerId,
    SwarmBuilder,
    identify,
    identity::{ self, Keypair },
    kad,
    noise,
    ping,
    relay,
    swarm::{ NetworkBehaviour, SwarmEvent },
    tcp,
    yamux,
};
use std::{ collections::HashMap, env, error::Error, fs, path::PathBuf };
use futures::StreamExt;

#[derive(NetworkBehaviour)]
struct RelayBehaviour {
    relay: relay::Behaviour,
    identify: identify::Behaviour,
    ping: ping::Behaviour,
    kademlia: kad::Behaviour<kad::store::MemoryStore>,
}

type PeerTable = HashMap<PeerId, Vec<Multiaddr>>;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    let public_ip = args.get(1).unwrap();

    let local_key = load_or_create_identity();
    let local_peer_id = libp2p::PeerId::from(local_key.public());

    #[cfg(debug_assertions)]
    tracing_subscriber::fmt().with_env_filter("libp2p=debug").init();

    let mut swarm = SwarmBuilder::with_existing_identity(local_key)
        .with_tokio()
        // 1. TRANSPORTE TCP (Necesario para compatibilidad de Relay v2)
        .with_tcp(tcp::Config::default(), noise::Config::new, yamux::Config::default)?
        // 2. TRANSPORTE QUIC (Para el "Buffet de Bytes" de alto rendi
        .with_quic()
        .with_behaviour(|key: &Keypair| {
            let peer_id = PeerId::from(key.public());

            let ping = ping::Behaviour::new(ping::Config::default());

            let mut kademlia = kad::Behaviour::new(peer_id, kad::store::MemoryStore::new(peer_id));
            kademlia.set_mode(Some(kad::Mode::Server));

            RelayBehaviour {
                relay: relay::Behaviour::new(local_peer_id, relay::Config::default()),
                identify: identify::Behaviour::new(
                    identify::Config::new("/knot/1.0.0".into(), key.public())
                ),
                ping,
                kademlia,
            }
        })?
        .with_swarm_config(|c| c.with_idle_connection_timeout(std::time::Duration::from_secs(60)))
        .build();

    // ESCUCHA EN AMBOS
    swarm.listen_on("/ip4/0.0.0.0/tcp/4001".parse()?)?; // Puerto TCP
    //swarm.listen_on("/ip4/0.0.0.0/udp/4001/quic-v1".parse()?)?;    // Puerto QUIC

    swarm.add_external_address(format!("/ip4/{}/tcp/4001", public_ip).parse().unwrap());

    println!("Knot Relay operativo en {}", local_peer_id);

    // table for saved peers
    let mut peer_table: PeerTable = HashMap::new();

    loop {
        match swarm.select_next_some().await {
            SwarmEvent::NewListenAddr { address, .. } => println!("Escuchando en: {address}"),
            SwarmEvent::Behaviour(
                RelayBehaviourEvent::Relay(
                    relay::Event::ReservationReqAccepted { src_peer_id, renewed },
                ),
            ) => {
                println!("New reservation: {} | renewed: {}", src_peer_id, renewed);
            }
            SwarmEvent::Behaviour(
                RelayBehaviourEvent::Relay(
                    relay::Event::CircuitReqAccepted { src_peer_id, dst_peer_id },
                ),
            ) => {
                println!("Circuit Accepted: {} -> {}", src_peer_id, dst_peer_id);
            }
            SwarmEvent::Behaviour(
                RelayBehaviourEvent::Relay(
                    relay::Event::ReservationReqDenied { src_peer_id, status: _ },
                ),
            ) => {
                println!("Denied reservation: {}", src_peer_id);
            }
            SwarmEvent::Behaviour(
                RelayBehaviourEvent::Relay(
                    relay::Event::CircuitReqDenied { src_peer_id, dst_peer_id, status },
                ),
            ) => {
                println!("Circuit Denied: {} -> {} - {:?}", src_peer_id, dst_peer_id, status);
            }

            SwarmEvent::Behaviour(
                RelayBehaviourEvent::Identify(
                    identify::Event::Received { peer_id, info, connection_id: _ },
                ),
            ) => {
                println!(
                    "[Network] Identify recibido de {}: agent={}",
                    peer_id,
                    info.agent_version
                );
                for addr in &info.listen_addrs {
                    swarm.behaviour_mut().kademlia.add_address(&peer_id, addr.clone());
                    peer_table.entry(peer_id).or_default().push(addr.clone());
                }
                // Lanzar bootstrap si es el primer peer conocido
                if peer_table.len() == 1 {
                    let _ = swarm.behaviour_mut().kademlia.bootstrap();
                    println!("[Network] Kademlia bootstrap iniciado");
                }
            }

            SwarmEvent::Behaviour(
                RelayBehaviourEvent::Kademlia(kad::Event::RoutingUpdated { peer, addresses, .. }),
            ) => {
                println!("[Network] DHT routing actualizado para {}", peer);
                for addr in addresses.iter() {
                    peer_table.entry(peer).or_default().push(addr.clone());
                }
            }
            SwarmEvent::Behaviour(
                RelayBehaviourEvent::Kademlia(
                    kad::Event::OutboundQueryProgressed {
                        result: kad::QueryResult::Bootstrap(
                            Ok(kad::BootstrapOk { num_remaining, .. }),
                        ),
                        ..
                    },
                ),
            ) => {
                if num_remaining == 0 {
                    println!("[Network] Bootstrap Kademlia completado");
                }
            }
            _ => {}
        }
    }
}

fn load_or_create_identity() -> identity::Keypair {
    let mut config_path = get_knot_config_dir();
    config_path.push("identity.bin");

    if config_path.exists() {
        let bytes = fs::read(&config_path).expect("No se pudo leer la identidad");
        identity::Keypair::from_protobuf_encoding(&bytes)
            .expect("Archivo de identidad corrupto")
    } else {
        let new_key = identity::Keypair::generate_ed25519();
        let encoded = new_key.to_protobuf_encoding().unwrap();
        
        fs::write(&config_path, encoded).expect("No se pudo guardar la identidad");
        println!("Identidad persistente creada en: {:?}", config_path);
        new_key
    }
}

fn get_knot_config_dir() -> PathBuf {
    // dirs::config_dir() devuelve:
    // Linux:   /home/usuario/.config
    // Windows: C:\Users\Usuario\AppData\Roaming
    let mut path = dirs::config_dir().expect("No se pudo encontrar el directorio de configuración");
    
    // Añadimos nuestra carpeta específica
    path.push("knot");
    
    // Nos aseguramos de que la carpeta exista (si no, fs::write fallará)
    if !path.exists() {
        fs::create_dir_all(&path).expect("No se pudo crear la carpeta de configuración de Knot");
    }
    
    path
}
