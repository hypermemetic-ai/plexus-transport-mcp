//! KEEL-59 · an MCP client that goes away is a cancel.
//!
//! rmcp runs each request's handler as a task of its own and cancels its
//! `ct` on `notifications/cancelled`; when the client's transport closes (a
//! streamable-HTTP session deleted or timed out, stdin closed) it first
//! waits up to five seconds for the handlers it is still running, and on a
//! session it never sees end it waits for ever. A call whose client is gone
//! has no one to answer, so here its end is a cancel, at once: the session's
//! transport is [`Watched`], which hands every request a [`ClientGone`]
//! (rmcp moves a request's extensions into its `RequestContext`) and cancels
//! it the moment the transport says it is closed — its `receive` answers
//! nothing more, or it is closed or dropped. A handler selects on it beside
//! `ctx.ct` ([`gone`]).
//!
//! An HTTP response dropped mid-call while its session is still open is not
//! the client going away: MCP's streamable HTTP lets the client resume the
//! stream (`Last-Event-ID`) and asks it to send `notifications/cancelled`
//! to cancel. That call ends at its cancel, its session's end, or the
//! bridge's own limit (`Limits::call_secs`).
//!
//! [`EndsCalls`] wraps a [`SessionManager`] so every session it makes is
//! watched — the mounted bridge's, and the legacy bridge's in plexus-transport.

use std::future::Future;

use futures::Stream;
use rmcp::model::{ClientJsonRpcMessage, GetExtensions, JsonRpcMessage, ServerJsonRpcMessage};
use rmcp::service::{RequestContext, RoleServer, RxJsonRpcMessage, TxJsonRpcMessage};
use rmcp::transport::streamable_http_server::session::{RestoreOutcome, ServerSseMessage, SessionId, SessionManager};
use rmcp::transport::Transport;
use tokio_util::sync::CancellationToken;

/// Cancelled when the client's transport has closed: in each request's
/// extensions, on a session [`Watched`].
#[derive(Clone, Debug)]
pub struct ClientGone(pub CancellationToken);

/// The request's [`ClientGone`], when its session is watched.
pub fn gone(ctx: &RequestContext<RoleServer>) -> Option<&CancellationToken> {
    ctx.extensions.get::<ClientGone>().map(|g| &g.0)
}

/// Resolves when the request's client has gone away (never, on a session
/// that is not watched).
pub async fn client_gone(ctx: &RequestContext<RoleServer>) {
    match gone(ctx) {
        Some(t) => t.cancelled().await,
        None => std::future::pending().await,
    }
}

/// A server transport whose end cancels every request it carried.
pub struct Watched<T> {
    inner: T,
    gone: CancellationToken,
}

impl<T> Watched<T> {
    pub fn new(inner: T) -> Self {
        Self { inner, gone: CancellationToken::new() }
    }

    /// Cancelled once the transport has closed.
    pub fn gone(&self) -> CancellationToken {
        self.gone.clone()
    }
}

impl<T> Drop for Watched<T> {
    fn drop(&mut self) {
        self.gone.cancel();
    }
}

impl<T: Transport<RoleServer>> Transport<RoleServer> for Watched<T> {
    type Error = T::Error;

    fn send(&mut self, item: TxJsonRpcMessage<RoleServer>) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
        self.inner.send(item)
    }

    fn receive(&mut self) -> impl Future<Output = Option<RxJsonRpcMessage<RoleServer>>> + Send {
        let gone = self.gone.clone();
        let next = self.inner.receive();
        async move {
            match next.await {
                Some(mut message) => {
                    mark(&mut message, &gone);
                    Some(message)
                }
                None => {
                    gone.cancel();
                    None
                }
            }
        }
    }

    fn close(&mut self) -> impl Future<Output = Result<(), Self::Error>> + Send {
        self.gone.cancel();
        self.inner.close()
    }
}

/// A request carries the session's [`ClientGone`].
fn mark(message: &mut ClientJsonRpcMessage, gone: &CancellationToken) {
    if let JsonRpcMessage::Request(r) = message {
        r.request.extensions_mut().insert(ClientGone(gone.clone()));
    }
}

/// A [`SessionManager`] whose every session's transport is [`Watched`].
pub struct EndsCalls<M>(pub M);

impl<M> EndsCalls<M> {
    pub fn new(inner: M) -> Self {
        Self(inner)
    }
}

impl<M: SessionManager> SessionManager for EndsCalls<M> {
    type Error = M::Error;
    type Transport = Watched<M::Transport>;

    async fn create_session(&self) -> Result<(SessionId, Self::Transport), Self::Error> {
        let (id, t) = self.0.create_session().await?;
        Ok((id, Watched::new(t)))
    }

    fn initialize_session(&self, id: &SessionId, message: ClientJsonRpcMessage) -> impl Future<Output = Result<ServerJsonRpcMessage, Self::Error>> + Send {
        self.0.initialize_session(id, message)
    }

    fn has_session(&self, id: &SessionId) -> impl Future<Output = Result<bool, Self::Error>> + Send {
        self.0.has_session(id)
    }

    fn close_session(&self, id: &SessionId) -> impl Future<Output = Result<(), Self::Error>> + Send {
        self.0.close_session(id)
    }

    fn create_stream(
        &self,
        id: &SessionId,
        message: ClientJsonRpcMessage,
    ) -> impl Future<Output = Result<impl Stream<Item = ServerSseMessage> + Send + Sync + 'static, Self::Error>> + Send {
        self.0.create_stream(id, message)
    }

    fn accept_message(&self, id: &SessionId, message: ClientJsonRpcMessage) -> impl Future<Output = Result<(), Self::Error>> + Send {
        self.0.accept_message(id, message)
    }

    fn create_standalone_stream(&self, id: &SessionId) -> impl Future<Output = Result<impl Stream<Item = ServerSseMessage> + Send + Sync + 'static, Self::Error>> + Send {
        self.0.create_standalone_stream(id)
    }

    fn resume(
        &self,
        id: &SessionId,
        last_event_id: String,
    ) -> impl Future<Output = Result<impl Stream<Item = ServerSseMessage> + Send + Sync + 'static, Self::Error>> + Send {
        self.0.resume(id, last_event_id)
    }

    async fn restore_session(&self, id: SessionId) -> Result<RestoreOutcome<Self::Transport>, Self::Error> {
        Ok(match self.0.restore_session(id).await? {
            RestoreOutcome::Restored(t) => RestoreOutcome::Restored(Watched::new(t)),
            RestoreOutcome::AlreadyPresent => RestoreOutcome::AlreadyPresent,
            // NotSupported, and any outcome a later rmcp adds: not restored.
            _ => RestoreOutcome::NotSupported,
        })
    }

    fn event_store(&self) -> Option<std::sync::Arc<dyn rmcp::transport::streamable_http_server::session::EventStore>> {
        self.0.event_store()
    }
}

#[cfg(test)]
mod tests {
    //! A client that goes away: every request its session carried is told.
    use super::*;
    use rmcp::model::{ClientRequest, JsonRpcRequest, NumberOrString, PingRequest, RequestId};
    use std::time::Duration;

    /// A session's transport that delivers `requests`, then says it closed.
    struct Session(Vec<ClientJsonRpcMessage>);

    #[derive(Debug, thiserror::Error)]
    #[error("never")]
    struct Never;

    impl Transport<RoleServer> for Session {
        type Error = Never;
        fn send(&mut self, _item: TxJsonRpcMessage<RoleServer>) -> impl Future<Output = Result<(), Never>> + Send + 'static {
            std::future::ready(Ok(()))
        }
        fn receive(&mut self) -> impl Future<Output = Option<RxJsonRpcMessage<RoleServer>>> + Send {
            std::future::ready(self.0.pop())
        }
        fn close(&mut self) -> impl Future<Output = Result<(), Never>> + Send {
            std::future::ready(Ok(()))
        }
    }

    fn ping(id: i64) -> ClientJsonRpcMessage {
        let id: RequestId = NumberOrString::Number(id);
        JsonRpcMessage::Request(JsonRpcRequest::new(id, ClientRequest::PingRequest(PingRequest::default())))
    }

    fn told(message: &ClientJsonRpcMessage) -> CancellationToken {
        match message {
            JsonRpcMessage::Request(r) => r.request.extensions().get::<ClientGone>().expect("a request carries its session's end").0.clone(),
            _ => panic!("a request"),
        }
    }

    #[tokio::test]
    async fn a_session_that_ends_cancels_every_request_it_carried() {
        let mut t = Watched::new(Session(vec![ping(2), ping(1)]));
        let first = told(&t.receive().await.unwrap());
        let second = told(&t.receive().await.unwrap());
        assert!(!first.is_cancelled() && !second.is_cancelled(), "the client is still there");
        assert!(t.receive().await.is_none(), "the session ended");
        tokio::time::timeout(Duration::from_millis(100), first.cancelled()).await.expect("the first request told at once");
        assert!(second.is_cancelled());
    }

    #[tokio::test]
    async fn a_session_closed_or_dropped_cancels_too() {
        let mut t = Watched::new(Session(vec![ping(1)]));
        let one = told(&t.receive().await.unwrap());
        t.close().await.unwrap();
        assert!(one.is_cancelled(), "closed");
        let mut t = Watched::new(Session(vec![ping(1)]));
        let one = told(&t.receive().await.unwrap());
        drop(t);
        assert!(one.is_cancelled(), "dropped");
    }
}
