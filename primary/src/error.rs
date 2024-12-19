// Copyright(C) Facebook, Inc. and its affiliates.
use crate::primary::Round;
use crypto::{CryptoError, Digest, PublicKey};
use store::StoreError;
use thiserror::Error;

// 定义一个宏，用于快速返回错误
#[macro_export]
macro_rules! bail {
    ($e:expr) => {
        return Err($e);
    };
}

// 定义一个宏，用于确保条件为真，否则返回错误
#[macro_export(local_inner_macros)]
macro_rules! ensure {
    ($cond:expr, $e:expr) => {
        if !($cond) {
            bail!($e);
        }
    };
}

// 定义一个通用的结果类型，用于封装成功值或'DagError'
pub type DagResult<T> = Result<T, DagError>;

// // 定义 `DagError` 枚举，用于表示 DAG 处理过程中可能发生的错误。
#[derive(Debug, Error)]
pub enum DagError {
    // 签名无效错误
    #[error("Invalid signature")]
    InvalidSignature(#[from] CryptoError),

    // 存储错误
    #[error("Storage failure: {0}")]
    StoreError(#[from] StoreError),

    // 序列化错误
    #[error("Serialization error: {0}")]
    SerializationError(#[from] Box<bincode::ErrorKind>),

    // 无效的 Header Id 错误
    #[error("Invalid header id")]
    InvalidHeaderId,

    // Header 格式错误
    #[error("Malformed header {0}")]
    MalformedHeader(Digest),

    // 受到来自未知权限的消息
    #[error("Received message from unknown authority {0}")]
    UnknownAuthority(PublicKey),

    // 权限在仲裁中出现多次
    #[error("Authority {0} appears in quorum more than once")]
    AuthorityReuse(PublicKey),

    // 收到意外的投票
    #[error("Received unexpected vote fo header {0}")]
    UnexpectedVote(Digest),

    // 收到没有仲裁的证书
    #[error("Received certificate without a quorum")]
    CertificateRequiresQuorum,

    // Header 的父项未达到仲裁要求
    #[error("Parents of header {0} are not a quorum")]
    HeaderRequiresQuorum(Digest),

    // 消息过旧错误
    #[error("Message {0} (round {1}) too old")]
    TooOld(Digest, Round),
}
