use kameo::{
    Actor,
    message::Message,
    request::{TellRequest, WithoutRequestTimeout},
};

/// 通用 fire-and-forget：将消息投入 mailbox，不等待处理
pub trait KameoExt {
    fn fire(self);
}

impl<A: Actor + Message<M>, M: Send + 'static> KameoExt
    for TellRequest<'_, A, M, WithoutRequestTimeout>
{
    fn fire(self) {
        if let Err(err) = self.try_send() {
            tracing::error!(
                "Failed to fire message to {}: {}",
                std::any::type_name::<A>(),
                err
            );
        }
    }
}
