//! Signs release checksums in the minisign format the updater checks.
//!
//!   tessera-sign gen <secret-key-file> <public-key-file>
//!   MINISIGN_SECRET_KEY=... tessera-sign sign <file> <trusted comment>  > <file>.minisig
//!
//! The secret key has no password; it lives only in the repository secret.

use std::io::Cursor;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("gen") if args.len() == 4 => {
            let pair = minisign::KeyPair::generate_encrypted_keypair(Some(String::new()))
                .expect("key generation failed");
            let secret = pair.sk.to_box(None).expect("secret key").to_string();
            std::fs::write(&args[2], secret).expect("writing the secret key");
            std::fs::write(&args[3], pair.pk.to_box().expect("public key").to_string())
                .expect("writing the public key");
            println!("{}", pair.pk.to_base64());
        }
        Some("sign") if args.len() == 4 => {
            let text = std::env::var("MINISIGN_SECRET_KEY").expect("MINISIGN_SECRET_KEY is not set");
            let secret = minisign::SecretKeyBox::from_string(&text)
                .and_then(|b| b.into_secret_key(Some(String::new())))
                .expect("the secret key cannot be read");
            let data = std::fs::read(&args[2]).expect("reading the file to sign");
            let signature = minisign::sign(None, &secret, Cursor::new(data), Some(&args[3]), None)
                .expect("signing failed");
            print!("{}", signature.into_string());
        }
        _ => {
            eprintln!("usage: tessera-sign gen <secret> <public> | sign <file> <comment>");
            std::process::exit(2);
        }
    }
}
