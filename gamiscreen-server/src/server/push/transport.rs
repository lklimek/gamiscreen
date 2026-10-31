use aws_lc_rs::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::Utc;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub(super) enum TransportError {
    #[error("invalid Web Push key encoding: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("VAPID key pair is invalid: {0}")]
    Key(#[from] aws_lc_rs::error::KeyRejected),
    #[error("VAPID key serialization failed: {0}")]
    KeyEncoding(#[from] aws_lc_rs::error::Unspecified),
    #[error("VAPID signature failed: {0}")]
    Jwt(#[from] jsonwebtoken::errors::Error),
    #[error("Web Push encryption failed: {0}")]
    Encryption(#[from] ece::Error),
    #[error("Web Push HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("push service returned HTTP {0}")]
    Status(reqwest::StatusCode),
}

impl TransportError {
    pub(super) fn is_stale_subscription(&self) -> bool {
        matches!(
            self,
            Self::Status(reqwest::StatusCode::NOT_FOUND | reqwest::StatusCode::GONE)
        )
    }
}

pub(super) struct PushTransport {
    client: reqwest::Client,
    signing_key: EncodingKey,
    public_key: String,
    contact: Option<String>,
}

#[derive(Serialize)]
struct VapidClaims<'a> {
    aud: String,
    exp: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    sub: Option<&'a str>,
}

impl PushTransport {
    pub(super) fn new(
        private: &str,
        public: &str,
        contact: Option<String>,
    ) -> Result<Self, TransportError> {
        let pair = EcdsaKeyPair::from_private_key_and_public_key(
            &ECDSA_P256_SHA256_FIXED_SIGNING,
            &URL_SAFE_NO_PAD.decode(private)?,
            &URL_SAFE_NO_PAD.decode(public)?,
        )?;
        Ok(Self {
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(std::time::Duration::from_secs(30))
                .build()?,
            signing_key: EncodingKey::from_ec_der(pair.to_pkcs8v1()?.as_ref()),
            public_key: public.to_owned(),
            contact,
        })
    }

    fn prepare_request(
        &self,
        endpoint: &str,
        p256dh: &str,
        auth: &str,
        payload: &[u8],
    ) -> Result<reqwest::Request, TransportError> {
        let encrypted = ece::encrypt(
            &URL_SAFE_NO_PAD.decode(p256dh)?,
            &URL_SAFE_NO_PAD.decode(auth)?,
            payload,
        )?;
        let url = self.client.post(endpoint).build()?.url().clone();
        let claims = VapidClaims {
            aud: url.origin().ascii_serialization(),
            exp: Utc::now().timestamp() + 12 * 60 * 60,
            sub: self.contact.as_deref(),
        };
        let token =
            jsonwebtoken::encode(&Header::new(Algorithm::ES256), &claims, &self.signing_key)?;
        // RFC 8292 authorization; reqwest validates header values before sending.
        Ok(self
            .client
            .post(url)
            .header(
                "authorization",
                format!("vapid t={token}, k={}", self.public_key),
            )
            .header("content-encoding", "aes128gcm")
            .header("content-type", "application/octet-stream")
            .header("ttl", "2419200")
            .body(encrypted)
            .build()?)
    }

    pub(super) async fn send(
        &self,
        endpoint: &str,
        p256dh: &str,
        auth: &str,
        payload: &[u8],
    ) -> Result<(), TransportError> {
        let response = self
            .client
            .execute(self.prepare_request(endpoint, p256dh, auth, payload)?)
            .await?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(TransportError::Status(response.status()))
        }
    }
}

#[cfg(test)]
mod tests {
    use aws_lc_rs::encoding::AsBigEndian;
    use aws_lc_rs::signature::KeyPair;
    use base64::Engine;
    use jsonwebtoken::{Algorithm, DecodingKey, Validation};
    use serde_json::Value;

    use super::*;

    fn transport() -> (PushTransport, Vec<u8>) {
        let rng = aws_lc_rs::rand::SystemRandom::new();
        let document =
            EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng).unwrap();
        let pair =
            EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, document.as_ref()).unwrap();
        let public = pair.public_key().as_ref().to_vec();
        let transport = PushTransport::new(
            &URL_SAFE_NO_PAD.encode(pair.private_key().as_be_bytes().unwrap().as_ref()),
            &URL_SAFE_NO_PAD.encode(&public),
            Some("mailto:admin@example.test".into()),
        )
        .unwrap();
        (transport, public)
    }

    #[test]
    fn request_is_encrypted_and_vapid_is_signed_for_the_push_origin() {
        let (transport, public) = transport();
        let (recipient, auth) = ece::generate_keypair_and_auth_secret().unwrap();
        let request = transport
            .prepare_request(
                "https://push.example.test:8443/delivery/123?x=1",
                &URL_SAFE_NO_PAD.encode(recipient.pub_as_raw().unwrap()),
                &URL_SAFE_NO_PAD.encode(auth),
                b"private event",
            )
            .unwrap();
        assert_eq!(request.method(), reqwest::Method::POST);
        assert_eq!(request.headers()["content-encoding"], "aes128gcm");
        assert_eq!(request.headers()["ttl"], "2419200");
        let body = request.body().unwrap().as_bytes().unwrap();
        assert_eq!(
            ece::decrypt(&recipient.raw_components().unwrap(), &auth, body).unwrap(),
            b"private event"
        );
        let authorization = request.headers()["authorization"].to_str().unwrap();
        let (token, key) = authorization
            .strip_prefix("vapid t=")
            .unwrap()
            .split_once(", k=")
            .unwrap();
        assert_eq!(URL_SAFE_NO_PAD.decode(key).unwrap(), public);
        let mut validation = Validation::new(Algorithm::ES256);
        validation.set_audience(&["https://push.example.test:8443"]);
        let claims =
            jsonwebtoken::decode::<Value>(token, &DecodingKey::from_ec_der(&public), &validation)
                .unwrap()
                .claims;
        assert_eq!(claims["sub"], "mailto:admin@example.test");
        let lifetime = claims["exp"].as_i64().unwrap() - Utc::now().timestamp();
        assert!((43_190..=43_200).contains(&lifetime));
    }

    #[test]
    fn malformed_subscription_keys_are_rejected() {
        let (transport, _) = transport();
        assert!(
            transport
                .prepare_request("https://push.example.test/send", "%%%", "%%%", b"event")
                .is_err()
        );
        assert!(PushTransport::new("%%%", "%%%", None).is_err());
    }

    #[tokio::test]
    async fn delivery_classifies_expired_subscriptions_and_does_not_follow_redirects() {
        use axum::Router;
        use axum::routing::post;
        for code in [201, 404, 410, 429, 503, 302] {
            let status = reqwest::StatusCode::from_u16(code).unwrap();
            let app = Router::new().route(
                "/push",
                post(
                    move || async move { (status, [("location", "http://127.0.0.1:1/redirect")]) },
                ),
            );
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = format!("http://{}/push", listener.local_addr().unwrap());
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let (transport, _) = transport();
            let (recipient, auth) = ece::generate_keypair_and_auth_secret().unwrap();
            let result = transport
                .send(
                    &endpoint,
                    &URL_SAFE_NO_PAD.encode(recipient.pub_as_raw().unwrap()),
                    &URL_SAFE_NO_PAD.encode(auth),
                    b"event",
                )
                .await;
            if code == 201 {
                assert!(result.is_ok());
            } else {
                let error = result.unwrap_err();
                assert_eq!(error.is_stale_subscription(), code == 404 || code == 410);
                assert!(matches!(error, TransportError::Status(s) if s == status));
            }
            server.abort();
        }
    }
}
