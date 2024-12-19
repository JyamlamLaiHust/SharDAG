// Copyright(C) Facebook, Inc. and its affiliates.
use crypto::{generate_production_keypair, PublicKey, SecretKey};
use log::info;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs::{self, OpenOptions};
use std::io::BufWriter;
use std::io::Write as _;
use std::net::SocketAddr;
use thiserror::Error;

// 定义错误类型
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("shard {0} is not in the committees")]
    NotInCommittees(ShardId), // 分片不在委员会中

    #[error("Node {0} is not in the committee")]
    NotInCommittee(PublicKey), // 节点不在委员会中

    #[error("Unknown worker id {0}")]
    UnknownWorker(WorkerId), // 未知的工作者ID

    #[error("Failed to read config file '{file}': {message}")]
    ImportError { file: String, message: String }, // 导入配置文件失败

    #[error("Failed to write config file '{file}': {message}")]
    ExportError { file: String, message: String }, // 导出配置文件失败
}

// 定义'Import'特性，用于从文件中加载配置
pub trait Import: DeserializeOwned {
    fn import(path: &str) -> Result<Self, ConfigError> {
        let reader = || -> Result<Self, std::io::Error> {
            let data = fs::read(path)?; // 从路径读取文件内容
            Ok(serde_json::from_slice(data.as_slice())?)
        };
        reader().map_err(|e| ConfigError::ImportError {
            file: path.to_string(),
            message: e.to_string(),
        })
    }
}

// 定义 `Export` 特性，用于将配置写入文件
pub trait Export: Serialize {
    fn export(&self, path: &str) -> Result<(), ConfigError> {
        let writer = || -> Result<(), std::io::Error> {
            let file = OpenOptions::new().create(true).write(true).open(path)?;
            let mut writer = BufWriter::new(file);
            let data = serde_json::to_string_pretty(self).unwrap();
            writer.write_all(data.as_ref())?;
            writer.write_all(b"\n")?;
            Ok(())
        };
        writer().map_err(|e| ConfigError::ExportError {
            file: path.to_string(),
            message: e.to_string(),
        })
    }
}

pub type Stake = usize; // 投票权类型
pub type WorkerId = u32; // 工作者 ID
pub type ShardId = usize; // 分片 ID
pub type NodeId = u32; // 节点 ID
pub type ShardNum = usize; // 分片数量

// 'Parameters' 定义系统参数，并提供默认值
#[derive(Deserialize, Clone)]
pub struct Parameters {
    /// The preferred header size. The primary creates a new header when it has enough parents and
    /// enough batches' digests to reach `header_size`. Denominated in bytes.
    pub header_size: usize,
    /// The maximum delay that the primary waits between generating two headers, even if the header
    /// did not reach `max_header_size`. Denominated in ms.
    pub max_header_delay: u64,
    /// The depth of the garbage collection (Denominated in number of rounds).
    pub gc_depth: u64, // 垃圾回收深度
    /// The delay after which the synchronizer retries to send sync requests. Denominated in ms.
    pub sync_retry_delay: u64,
    /// Determine with how many nodes to sync when re-trying to send sync-request. These nodes
    /// are picked at random from the committee.
    pub sync_retry_nodes: usize,
    /// The preferred batch size. The workers seal a batch of transactions when it reaches this size.
    /// Denominated in bytes.
    pub batch_size: usize,
    /// The delay after which the workers seal a batch of transactions, even if `max_batch_size`
    /// is not reached. Denominated in ms.
    pub max_batch_delay: u64,
}

impl Default for Parameters {
    fn default() -> Self {
        Self {
            header_size: 1_000,
            max_header_delay: 100,
            gc_depth: 50,
            sync_retry_delay: 5_000,
            sync_retry_nodes: 3,
            batch_size: 500_000,
            max_batch_delay: 100,
        }
    }
}

impl Import for Parameters {}

impl Parameters {
    pub fn log(&self) {
        info!("Header size set to {} B", self.header_size);
        info!("Max header delay set to {} ms", self.max_header_delay);
        info!("Garbage collection depth set to {} rounds", self.gc_depth);
        info!("Sync retry delay set to {} ms", self.sync_retry_delay);
        info!("Sync retry nodes set to {} nodes", self.sync_retry_nodes);
        info!("Batch size set to {} B", self.batch_size);
        info!("Max batch delay set to {} ms", self.max_batch_delay);
    }
}

#[derive(Clone, Deserialize, Debug)]
pub struct PrimaryAddresses {
    /// Address to receive messages from other primaries (WAN).
    pub primary_to_primary: SocketAddr,
    /// Address to receive messages from our workers (LAN).
    pub worker_to_primary: SocketAddr,
}

#[derive(Clone, Deserialize, Eq, Hash, PartialEq, Debug)]
pub struct WorkerAddresses {
    /// Address to receive client transactions (WAN).
    pub transactions: SocketAddr,
    /// Address to receive messages from other workers (WAN).
    pub worker_to_worker: SocketAddr,
    /// Address to receive messages from our primary (LAN).
    pub primary_to_worker: SocketAddr,
    /// Address to receive messages from other workers in other shards(WAN).
    pub cross_shard_worker: SocketAddr,
}

// 定义权限结构体，用于存储节点的投票权和地址信息
#[derive(Clone, Deserialize, Debug)]
pub struct Authority {
    /// The voting power of this authority.
    pub stake: Stake,
    /// The network addresses of the primary.
    pub primary: PrimaryAddresses,
    /// Map of workers' id and their network addresses.
    pub workers: HashMap<WorkerId, WorkerAddresses>,
}

// 定义委员会结构体，用于管理节点及其权限信息
#[derive(Clone, Deserialize, Debug)]
pub struct Committee {
    pub authorities: BTreeMap<PublicKey, Authority>,
}

impl Import for Committee {}


impl Committee {
    /// Returns the number of authorities.
    pub fn size(&self) -> usize {
        self.authorities.len() // 返回节点数量
    }

    /// Return the stake of a specific authority.
    pub fn stake(&self, name: &PublicKey) -> Stake {
        self.authorities.get(name).map_or_else(|| 0, |x| x.stake) // 查询某节点的投票权
    }

    /// Returns the stake of all authorities except `myself`.
    pub fn others_stake(&self, myself: &PublicKey) -> Vec<(PublicKey, Stake)> {
        self.authorities
            .iter()
            .filter(|(name, _)| name != &myself) // 排除自身节点
            .map(|(name, authority)| (*name, authority.stake))  // 收集其他节点的投票权
            .collect()
    }

    /// Returns the stake required to reach a quorum (2f+1).
    pub fn quorum_threshold(&self) -> Stake {
        // If N = 3f + 1 + k (0 <= k < 3)
        // then (2 N + 3) / 3 = 2f + 1 + (2k + 2)/3 = 2f + 1 + k = N - f
        let total_votes: Stake = self.authorities.values().map(|x| x.stake).sum();
        2 * total_votes / 3 + 1 // 返回达到法定人数所需投票权（2f + 1）
    }

    /// Returns the stake required to reach availability (f+1).
    pub fn validity_threshold(&self) -> Stake {
        // If N = 3f + 1 + k (0 <= k < 3)
        // then (N + 2) / 3 = f + 1 + k/3 = f + 1
        let total_votes: Stake = self.authorities.values().map(|x| x.stake).sum();
        (total_votes + 2) / 3 // 返回达到有效性的最小投票权（f+1）
    }

    /// Returns a leader node in a round-robin fashion.
    pub fn leader(&self, seed: usize) -> PublicKey {
        let mut keys: Vec<_> = self.authorities.keys().cloned().collect(); // 提取所有公钥
        keys.sort(); // 排序实现轮询机制
        keys[seed % self.size()] // 返回当前轮次的领导者
    }

    /// Returns the primary addresses of the target primary.
    pub fn primary(&self, to: &PublicKey) -> Result<PrimaryAddresses, ConfigError> {
        self.authorities
            .get(to)
            .map(|x| x.primary.clone()) // 查找制定节点的主节点地址
            .ok_or_else(|| ConfigError::NotInCommittee(*to)) // 节点不存在则返回 NotInCommittee
    }

    /// Returns the addresses of all primaries except `myself`.
    pub fn others_primaries(&self, myself: &PublicKey) -> Vec<(PublicKey, PrimaryAddresses)> {
        self.authorities
            .iter()
            .filter(|(name, _)| name != &myself) // 排除自身
            .map(|(name, authority)| (*name, authority.primary.clone())) // 获取其他主节点的地址
            .collect()
    }

    /// Returns the addresses of a specific worker (`id`) of a specific authority (`to`).
    pub fn worker(&self, to: &PublicKey, id: &WorkerId) -> Result<WorkerAddresses, ConfigError> {
        self.authorities
            .iter()
            .find(|(name, _)| name == &to)
            .map(|(_, authority)| authority)
            .ok_or_else(|| ConfigError::NotInCommittee(*to))?
            .workers
            .iter()
            .find(|(worker_id, _)| worker_id == &id)
            .map(|(_, worker)| worker.clone())
            .ok_or_else(|| ConfigError::NotInCommittee(*to))
    }

    /// Returns the addresses of all our workers.
    pub fn our_workers(&self, myself: &PublicKey) -> Result<Vec<WorkerAddresses>, ConfigError> {
        self.authorities
            .iter()
            .find(|(name, _)| name == &myself)
            .map(|(_, authority)| authority)
            .ok_or_else(|| ConfigError::NotInCommittee(*myself))?
            .workers
            .values()
            .cloned()
            .map(Ok)
            .collect()
    }

    /// Returns the addresses of all workers with a specific id except the ones of the authority
    /// specified by `myself`.
    pub fn others_workers(
        &self,
        myself: &PublicKey,
        id: &WorkerId,
    ) -> Vec<(PublicKey, WorkerAddresses)> {
        self.authorities
            .iter()
            .filter(|(name, _)| name != &myself)
            .filter_map(|(name, authority)| {
                authority
                    .workers
                    .iter()
                    .find(|(worker_id, _)| worker_id == &id)
                    .map(|(_, addresses)| (*name, addresses.clone()))
            })
            .collect()
    }

    /// Returns the addresses of all workers with a specific id
    pub fn all_worker_nodes(
      &self,
      id: &WorkerId,
    ) -> Vec<(PublicKey, WorkerAddresses)> {
        self.authorities
        .iter()
        .filter_map(|(name, authority)| {
            authority
                .workers
                .iter()
                .find(|(worker_id, _)| worker_id == &id)
                .map(|(_, addresses)| (*name, addresses.clone()))
        })
        .collect()
    }

}


// 定义多分片委员会结构体，用于支持分片
#[derive(Clone, Deserialize, Debug)]
pub struct Committees {
    pub shards: HashMap<ShardId, Committee>,
    pub client: SocketAddr,
    pub shard_num: usize,
    pub shard_size: usize,
}

impl Import for Committees {}

impl Committees {
    /// Return the committee of a specific ShardId.
    pub fn our_committee(&self, id: &ShardId) -> Result<Committee, ConfigError> {
      self.shards
      .iter()
      .find(|(shard_id, _)| shard_id == &id)
      .map(|(_, committee)| committee.clone())
      .ok_or_else(|| ConfigError::NotInCommittees(*id))            
    }

    pub fn shard_num(&self) -> usize {
        self.shards.len()
    }

    pub fn validity_threshold(&self) -> Stake {
      let committee_0 = self.shards.get(&0).unwrap();
      committee_0.validity_threshold()
    }

    pub fn shard_size(&self) -> usize {
      let committee_0 = self.shards.get(&0).unwrap();
      committee_0.size()
    }
}


#[derive(Serialize, Deserialize)]
pub struct KeyPair {
    /// The node's public key (and identifier).
    pub name: PublicKey,
    /// The node's secret key.
    pub secret: SecretKey,
}

impl Import for KeyPair {}
impl Export for KeyPair {}

impl KeyPair {
    pub fn new() -> Self {
        let (name, secret) = generate_production_keypair();
        Self { name, secret }
    }
}

impl Default for KeyPair {
    fn default() -> Self {
        Self::new()
    }
}
