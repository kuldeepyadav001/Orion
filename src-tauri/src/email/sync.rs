//! Inbound Email Sync & Extraction Engine (Milestone 7).
//!
//! Enforces:
//! 1. Rule R-5: Inbound reading & triage only. Strictly no automated outbound transmission.
//! 2. Domain::Untrusted quarantine: Every inbound email body is sanitized through `sanitize_email_body`.
//! 3. Resilient ingestion: Handles live IMAP over TLS as well as offline `.eml` file imports.

use std::path::Path;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::Mutex;

use crate::db::Db;
use crate::email::{sanitize_email_body, triage_email_content};
use crate::error::{OrionError, Result};

/// Configuration for connecting to an IMAP mail server.
#[derive(Debug, Clone)]
pub struct ImapConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_token: String,
}

/// Recursively extract the most relevant text body (plain text preferred, then HTML) from a parsed MIME email.
fn extract_body_from_parsed_mail(mail: &mailparse::ParsedMail) -> String {
    if mail.subparts.is_empty() {
        return mail.get_body().unwrap_or_default();
    }

    // 1. Look for text/plain parts first
    for part in &mail.subparts {
        if part.ctype.mimetype.eq_ignore_ascii_case("text/plain") {
            let body = part.get_body().unwrap_or_default();
            if !body.trim().is_empty() {
                return body;
            }
        }
    }

    // 2. Fall back to text/html
    for part in &mail.subparts {
        if part.ctype.mimetype.eq_ignore_ascii_case("text/html") {
            let body = part.get_body().unwrap_or_default();
            if !body.trim().is_empty() {
                return body;
            }
        }
    }

    // 3. Recursive inspection for nested multipart structures
    for part in &mail.subparts {
        let nested = extract_body_from_parsed_mail(part);
        if !nested.trim().is_empty() {
            return nested;
        }
    }

    mail.get_body().unwrap_or_default()
}

/// Parse, sanitize, and store an RFC822 email payload (used by both IMAP and .eml imports).
pub fn process_and_store_email_bytes(
    raw_bytes: &[u8],
    account_id: &str,
    source_label: &str,
    db: &Db,
) -> Result<String> {
    let parsed = mailparse::parse_mail(raw_bytes)
        .map_err(|e| OrionError::Config(format!("Failed to parse email structure: {e}")))?;

    let subject = parsed
        .headers
        .iter()
        .find(|h| h.get_key().eq_ignore_ascii_case("Subject"))
        .map(|h| h.get_value())
        .unwrap_or_else(|| "(No Subject)".into());

    let sender = parsed
        .headers
        .iter()
        .find(|h| h.get_key().eq_ignore_ascii_case("From"))
        .map(|h| h.get_value())
        .unwrap_or_else(|| "unknown@sender.com".into());

    let recipients = parsed
        .headers
        .iter()
        .find(|h| h.get_key().eq_ignore_ascii_case("To"))
        .map(|h| h.get_value())
        .unwrap_or_else(|| "me@localhost".into());

    let message_id = parsed
        .headers
        .iter()
        .find(|h| h.get_key().eq_ignore_ascii_case("Message-ID"))
        .map(|h| h.get_value())
        .unwrap_or_else(|| format!("<{}@orion.internal>", uuid::Uuid::new_v4()));

    let raw_body = extract_body_from_parsed_mail(&parsed);

    // Multi-Vector Sanitization & Prompt-Injection Quarantine
    let sanitized_body = sanitize_email_body(&raw_body);
    let triage = triage_email_content(&subject, &sanitized_body, &sender);

    // Extract sender display name or fallback
    let sender_name = if let Some(idx) = sender.find('<') {
        sender[..idx].trim().trim_matches('"').to_string()
    } else {
        sender.clone()
    };

    let email_id = format!("mail_{}", uuid::Uuid::new_v4().simple());
    let action_items_json = serde_json::to_string(&triage.action_items).unwrap_or_default();
    let now = chrono::Utc::now().to_rfc3339();

    db.insert_email(
        &email_id,
        account_id,
        &message_id,
        &sender,
        &sender_name,
        &recipients,
        &subject,
        &raw_body,
        &sanitized_body,
        &triage.category,
        triage.priority,
        &triage.reason,
        &action_items_json,
        &now,
    )?;

    tracing::info!(
        id = %email_id,
        source = %source_label,
        subject = %subject,
        category = %triage.category,
        "Ingested email into sovereign quarantine"
    );

    Ok(email_id)
}

/// Import a local .eml or RFC822 file into Orion's sovereign quarantine.
pub fn import_local_eml(path: &Path, account_id: &str, db: &Db) -> Result<String> {
    let bytes = std::fs::read(path)
        .map_err(|e| OrionError::Config(format!("Cannot read email file {}: {e}", path.display())))?;
    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("email.eml");
    process_and_store_email_bytes(&bytes, account_id, filename, db)
}

/// Synchronize recent emails from an IMAP server over TLS (Port 993).
pub async fn sync_inbox_via_imap(
    config: ImapConfig,
    account_id: &str,
    db: Arc<Mutex<Db>>,
    limit: usize,
) -> Result<usize> {
    // 1. Establish TCP socket connection
    let addr = format!("{}:{}", config.host, config.port);
    let tcp = TcpStream::connect(&addr)
        .await
        .map_err(|e| OrionError::Engine(format!("Failed to connect to {addr}: {e}")))?;

    // 2. Establish TLS Handshake
    let tls = native_tls::TlsConnector::builder()
        .build()
        .map_err(|e| OrionError::Engine(format!("TLS initialization error: {e}")))?;
    let tokio_tls = tokio_native_tls::TlsConnector::from(tls);
    let tls_stream = tokio_tls
        .connect(&config.host, tcp)
        .await
        .map_err(|e| OrionError::Engine(format!("TLS handshake with {} failed: {e}", config.host)))?;

    // 3. Connect IMAP client and authenticate
    let client = async_imap::Client::new(tls_stream);
    let mut session = client
        .login(&config.username, &config.auth_token)
        .await
        .map_err(|(e, _)| OrionError::Engine(format!("IMAP authentication rejected: {e}")))?;

    // 4. Select INBOX
    session
        .select("INBOX")
        .await
        .map_err(|e| OrionError::Engine(format!("Failed to select INBOX: {e}")))?;

    // 5. Search message UIDs
    let uids = session
        .search("ALL")
        .await
        .map_err(|e| OrionError::Engine(format!("IMAP search failed: {e}")))?;
    let mut uids_vec: Vec<u32> = uids.into_iter().collect();
    uids_vec.sort_unstable();

    let fetch_uids = if uids_vec.len() > limit {
        &uids_vec[uids_vec.len() - limit..]
    } else {
        &uids_vec[..]
    };

    let mut ingested_count = 0;

    for &uid in fetch_uids {
        let mut messages = session
            .fetch(uid.to_string(), "RFC822")
            .await
            .map_err(|e| OrionError::Engine(format!("IMAP fetch failed for UID {uid}: {e}")))?;

        while let Some(msg_res) = messages.next().await {
            if let Ok(msg) = msg_res {
                if let Some(body_bytes) = msg.body() {
                    let db_lock = db.lock().await;
                    let source_label = format!("imap:{}", uid);
                    if let Ok(_) = process_and_store_email_bytes(body_bytes, account_id, &source_label, &db_lock) {
                        ingested_count += 1;
                    }
                }
            }
        }
    }

    session.logout().await.map_err(|(e, _)| OrionError::Engine(e.to_string()))?;
    Ok(ingested_count)
}
