use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use time::{Duration, OffsetDateTime};

use crate::Result;

pub(crate) fn create(directory: &Path, server_name: &str) -> Result<()> {
    if server_name.is_empty()
        || server_name.len() > 253
        || !server_name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-'))
        || !server_name.as_bytes()[0].is_ascii_alphanumeric()
    {
        return Err("invalid certificate server name".into());
    }
    let now = OffsetDateTime::now_utc();
    let mut ca = CertificateParams::default();
    ca.not_before = now - Duration::minutes(5);
    ca.not_after = now + Duration::days(730);
    ca.distinguished_name
        .push(DnType::CommonName, "IW4L Community CA");
    ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let ca_key = KeyPair::generate()?;
    let ca_certificate = ca.self_signed(&ca_key)?;
    let issuer = Issuer::new(ca, ca_key);
    let mut server = CertificateParams::new(vec![server_name.to_owned()])?;
    server.not_before = now - Duration::minutes(5);
    server.not_after = now + Duration::days(729);
    server
        .distinguished_name
        .push(DnType::CommonName, server_name);
    server.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    server.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let key = KeyPair::generate()?;
    let certificate = server.signed_by(&key, &issuer)?;

    fs::create_dir_all(directory)?;
    let lock_path = directory.join("identity.lock");
    let lock = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_path)?;
    let files = [
        ("iw4l-ca.pem", ca_certificate.pem()),
        ("server-cert.pem", certificate.pem()),
        ("server-key.pem", key.serialize_pem()),
    ];
    let mut created = Vec::new();
    let result = (|| -> Result<()> {
        if files.iter().any(|(name, _)| directory.join(name).exists()) {
            return Err(
                "certificate files already exist; choose a new directory to preserve them".into(),
            );
        }
        for (name, pem) in &files {
            let path = directory.join(name);
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            created.push(path);
            file.write_all(pem.as_bytes())?;
            file.sync_all()?;
        }
        Ok(())
    })();
    if result.is_err() {
        for path in created {
            let _ = fs::remove_file(path);
        }
    }
    drop(lock);
    let _ = fs::remove_file(lock_path);
    result?;
    println!("Relay identity created in {}", directory.display());
    Ok(())
}
