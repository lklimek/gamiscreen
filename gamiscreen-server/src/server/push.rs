use std::{collections::HashMap, sync::Arc};

use chrono::{DateTime, Utc};
use gamiscreen_shared::api::ServerEvent;
use tokio::sync::Mutex;
use tracing::{info, warn};
use web_push::{
    ContentEncoding, IsahcWebPushClient, SubscriptionInfo, VapidSignatureBuilder, WebPushClient,
    WebPushMessageBuilder,
};

use crate::storage::{Store, models::PushSubscription};

use super::config::AppConfig;

#[derive(Clone)]
pub struct PushService {
    inner: Arc<PushServiceInner>,
}

struct PushServiceInner {
    client: IsahcWebPushClient,
    vapid_private: String,
    contact: Option<String>,
    tenant_id: String,
    recent_remaining: Mutex<HashMap<String, RemainingSnapshot>>,
}

struct RemainingSnapshot {
    remaining: i32,
    at: DateTime<Utc>,
    last_send: Option<DateTime<Utc>>,
}

impl PushService {
    pub fn from_config(cfg: &AppConfig) -> Option<Self> {
        let push_cfg = cfg.push.as_ref()?;
        if !push_cfg.enabled {
            return None;
        }
        let vapid_private = push_cfg.vapid_private.clone()?;
        if push_cfg
            .vapid_public
            .as_deref()
            .map(|s| s.trim().is_empty())
            .unwrap_or(true)
            || vapid_private.trim().is_empty()
        {
            warn!("push: missing VAPID key(s); disabling push service");
            return None;
        }

        let client = match IsahcWebPushClient::new() {
            Ok(c) => c,
            Err(err) => {
                warn!(error = %err, "push: failed to build HTTP client; disabling push service");
                return None;
            }
        };

        Some(Self {
            inner: Arc::new(PushServiceInner {
                client,
                vapid_private,
                contact: push_cfg.contact_email.clone(),
                tenant_id: cfg.tenant_id.clone(),
                recent_remaining: Mutex::new(HashMap::new()),
            }),
        })
    }

    pub fn dispatch_event(&self, store: Store, event: ServerEvent) {
        let inner = self.inner.clone();
        tokio::spawn(async move {
            if let Err(err) = inner.handle_event(store, event).await {
                warn!(error = %err, "push: failed to handle event");
            }
        });
    }
}

impl PushServiceInner {
    async fn handle_event(self: Arc<Self>, store: Store, event: ServerEvent) -> Result<(), String> {
        match &event {
            ServerEvent::RemainingUpdated {
                child_id,
                remaining_minutes,
            } => {
                if *remaining_minutes <= 0 {
                    return Ok(());
                }
                if !self
                    .should_push_remaining(child_id, *remaining_minutes)
                    .await
                {
                    return Ok(());
                }
                let subs = store
                    .list_push_subscriptions_for_child(&self.tenant_id, child_id)
                    .await?;
                if subs.is_empty() {
                    return Ok(());
                }
                let payload = Arc::new(serde_json::to_vec(&event).map_err(|e| e.to_string())?);
                self.send_to_subscriptions(store, subs, payload).await
            }
            ServerEvent::PendingCount { .. } => {
                let subs = store.list_all_push_subscriptions(&self.tenant_id).await?;
                if subs.is_empty() {
                    return Ok(());
                }
                let payload = Arc::new(serde_json::to_vec(&event).map_err(|e| e.to_string())?);
                self.send_to_subscriptions(store, subs, payload).await
            }
        }
    }

    async fn send_to_subscriptions(
        self: Arc<Self>,
        store: Store,
        subscriptions: Vec<PushSubscription>,
        payload: Arc<Vec<u8>>,
    ) -> Result<(), String> {
        for sub in subscriptions {
            let store_clone = store.clone();
            let payload_clone = payload.clone();
            let service = self.clone();
            tokio::spawn(async move {
                if let Err(err) = service.send_single(store_clone, sub, payload_clone).await {
                    warn!(error = %err, "push: send attempt failed");
                }
            });
        }
        Ok(())
    }

    async fn send_single(
        &self,
        store: Store,
        subscription: PushSubscription,
        payload: Arc<Vec<u8>>,
    ) -> Result<(), String> {
        let endpoint = subscription.endpoint.clone();

        let subscription_info = SubscriptionInfo::new(
            subscription.endpoint.clone(),
            subscription.p256dh.clone(),
            subscription.auth.clone(),
        );

        let mut builder = WebPushMessageBuilder::new(&subscription_info);
        builder.set_payload(ContentEncoding::Aes128Gcm, payload.as_ref());

        let mut vapid = VapidSignatureBuilder::from_base64(&self.vapid_private, &subscription_info)
            .map_err(|e| e.to_string())?;
        if let Some(contact) = &self.contact {
            vapid.add_claim("sub", contact.clone());
        }
        let signature = vapid.build().map_err(|e| e.to_string())?;
        builder.set_vapid_signature(signature);

        match self
            .client
            .send(builder.build().map_err(|e| e.to_string())?)
            .await
        {
            Ok(()) => {
                info!(endpoint = %endpoint, "push: delivered");
                if let Err(e) = store
                    .mark_push_delivery_result(subscription.id, true, None)
                    .await
                {
                    warn!(endpoint = %endpoint, error = %e, "push: failed to mark success");
                }
            }
            Err(err) => {
                let err_str = err.to_string();
                warn!(endpoint = %endpoint, error = %err_str, "push: send failed");

                if let Err(e) = store
                    .mark_push_delivery_result(subscription.id, false, Some(&err_str))
                    .await
                {
                    warn!(endpoint = %endpoint, error = %e, "push: failed to mark error");
                }

                if matches!(
                    err,
                    web_push::WebPushError::EndpointNotFound(_)
                        | web_push::WebPushError::EndpointNotValid(_)
                ) {
                    if let Err(e) = store
                        .delete_push_subscription(
                            &self.tenant_id,
                            &subscription.child_id,
                            &endpoint,
                        )
                        .await
                    {
                        warn!(
                            endpoint = %endpoint,
                            error = %e,
                            "push: failed to remove stale subscription"
                        );
                    }
                }

                return Err(err_str);
            }
        }

        Ok(())
    }
}

impl PushServiceInner {
    async fn should_push_remaining(&self, child_id: &str, remaining: i32) -> bool {
        let mut map = self.recent_remaining.lock().await;
        let now = Utc::now();
        match map.get(child_id) {
            None => {
                map.insert(
                    child_id.to_string(),
                    RemainingSnapshot {
                        remaining,
                        at: now,
                        last_send: Some(now),
                    },
                );
                true
            }
            Some(prev) => {
                let prev_remaining = prev.remaining;
                let prev_time = prev.at;
                let last_send = prev.last_send;
                // Expected drop based on elapsed wall-clock minutes
                let elapsed_secs = (now - prev_time).num_seconds().max(0) as f64;
                let elapsed_minutes = elapsed_secs / 60.0;
                let drop = (prev_remaining - remaining) as f64;
                let expected_drop = elapsed_minutes;
                let tolerance = (expected_drop * 0.05).max(0.25); // 5% tolerance, minimum 0.25 minute (~15s)

                let increased = remaining > prev_remaining;
                let out_of_range = (drop - expected_drop).abs() > tolerance;

                let mut snapshot = RemainingSnapshot {
                    remaining,
                    at: now,
                    last_send,
                };

                // Allow decreased notifications only if last push was sufficiently long ago (>=1 minute)
                let allow_drop = out_of_range
                    && match last_send {
                        Some(ts) => (now - ts).num_minutes() >= 1,
                        None => true,
                    };

                if increased || allow_drop {
                    snapshot.last_send = Some(now);
                    map.insert(child_id.to_string(), snapshot);
                    true
                } else {
                    // Update snapshot timestamps so elapsed time remains accurate, but keep last_send unchanged.
                    snapshot.last_send = last_send;
                    map.insert(child_id.to_string(), snapshot);
                    false
                }
            }
        }
    }
}
