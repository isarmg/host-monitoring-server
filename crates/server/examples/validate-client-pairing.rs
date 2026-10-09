use std::io::Read;

fn main() {
    let mut bytes = Vec::new();
    std::io::stdin()
        .read_to_end(&mut bytes)
        .expect("read Client pairing JSON from stdin");
    let request: xsos_protocol::ClientPairingRequest =
        serde_json::from_slice(&bytes).expect("deserialize current Client pairing JSON");
    xsos::model::validate_pairing(&request).expect("validate current Client pairing JSON");
}
