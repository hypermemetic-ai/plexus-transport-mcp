//! The fixture backend every test fronts: a gated service (`tickets`, roles
//! viewer and editor over the AMP shortcode), served on plexus-transport's
//! real WebSocket, anonymous upgrades admitted into the view.
#![allow(dead_code)]

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_stream::stream;
use async_trait::async_trait;
use futures::Stream;
use plexus_core::plexus::{AuthContext, Gate, Membership, Memberships, Resolve, RoleTable, ScopeRef, SessionValidator};
use plexus_core::DynamicHub;
use plexus_transport::websocket::{serve_websocket_with, UpgradeOptions};
use plexus_transport::WebSocketConfig;
use serde_json::Value;

#[derive(Clone)]
struct TicketsActivation;

#[plexus_macros::activation(
    version = "1.0.0",
    description = "TRANSPORT-16 fixture",
    roles(viewer = ["get", "list", "tick"], editor = [viewer, "set_status"]),
)]
impl TicketsActivation {
    /// One ticket.
    #[plexus_macros::method(target = "id", http_method = "GET")]
    async fn get(&self, id: String) -> impl Stream<Item = String> + Send + 'static {
        stream! { yield format!("ticket {id}"); }
    }

    /// Every ticket.
    #[plexus_macros::method(http_method = "GET")]
    async fn list(&self) -> impl Stream<Item = String> + Send + 'static {
        stream! { yield "AMP-1".to_string(); yield "AMP-2".to_string(); }
    }

    /// Change a status.
    #[plexus_macros::method(target = "id")]
    async fn set_status(&self, id: String, status: String) -> impl Stream<Item = String> + Send + 'static {
        stream! { yield format!("{id} -> {status}"); }
    }

    /// Listed by no role.
    #[plexus_macros::method(target = "id")]
    async fn delete(&self, id: String) -> impl Stream<Item = String> + Send + 'static {
        stream! { yield id; }
    }

    /// The door.
    #[plexus_macros::method(public)]
    async fn door(&self) -> impl Stream<Item = String> + Send + 'static {
        stream! { yield "in".to_string(); }
    }

    /// Never ends.
    #[plexus_macros::method(streaming = true)]
    async fn tick(&self) -> impl Stream<Item = String> + Send + 'static {
        stream! {
            let mut n = 0u64;
            loop {
                n += 1;
                yield format!("tick {n}");
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        }
    }
}

const ADA: &str = "key:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const CAT: &str = "key:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

#[derive(Default)]
struct Rows(Mutex<Vec<Membership>>);
impl Rows {
    fn add(&self, who: &str, role: &str, scope: &str) {
        self.0.lock().unwrap().push(Membership {
            principal: who.parse().unwrap(),
            role: plexus_auth_core::RoleName::new(role),
            scope: ScopeRef::parse(scope),
        });
    }
}
impl Memberships for Rows {
    fn held(&self, who: &plexus_auth_core::identity::Principal) -> Vec<Membership> {
        self.0.lock().unwrap().iter().filter(|m| &m.principal == who).cloned().collect()
    }
}

struct Shortcodes;
impl Resolve for Shortcodes {
    fn chain(&self, target: &str) -> Option<Vec<ScopeRef>> {
        let code = target.split_once('-').map(|(c, _)| c).unwrap_or(target);
        Some(vec![ScopeRef::named(code), ScopeRef::Everywhere])
    }
}

/// `ada` is an editor, `cat` a viewer; anything else is nobody.
struct Keys;
#[async_trait]
impl SessionValidator for Keys {
    async fn validate(&self, value: &str) -> Option<AuthContext> {
        let principal = match value {
            "ada" => ADA,
            "cat" => CAT,
            _ => return None,
        };
        Some(AuthContext::for_principal(&principal.parse().unwrap(), "s1", vec![], Value::Null))
    }
}

fn service() -> Arc<DynamicHub> {
    let rows = Arc::new(Rows::default());
    rows.add(ADA, "editor", "AMP");
    rows.add(CAT, "viewer", "AMP");
    let declared = DynamicHub::new("x").register(TicketsActivation).declared_roles();
    let roles = RoleTable::from_lists(declared.iter().map(|(r, g)| (r.as_str(), g.iter().map(String::as_str))));
    let gate = Gate::new(rows, Arc::new(roles), Arc::new(Shortcodes));
    Arc::new(DynamicHub::new("menger").register(TicketsActivation).with_gate(Arc::new(gate)))
}

/// The backend on its WebSocket, anonymous upgrades admitted into the view.
pub async fn backend() -> SocketAddr {
    let module = DynamicHub::arc_into_rpc_module(service()).unwrap();
    let upgrade = UpgradeOptions { anonymous_view: true, ..Default::default() };
    let (handle, addr) = serve_websocket_with(module, WebSocketConfig::new(0), Some(Arc::new(Keys)), upgrade).await.unwrap();
    // jsonrpsee stops the server when its last handle drops; the backend
    // lives as long as this test process.
    std::mem::forget(handle);
    addr
}

