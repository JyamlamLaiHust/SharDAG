// Copyright(C) Facebook, Inc. and its affiliates.
use crate::error::{DagError, DagResult};
use crate::messages::{Certificate, Header, Vote};
use config::{Committee, Stake};
use crypto::{PublicKey, Signature};
use std::collections::HashSet;

/// Aggregates votes for a particular header into a certificate.
// 为特定头部聚合投票以生成证书
pub struct VotesAggregator {
    weight: Stake, // 当前已聚合的总权益
    votes: Vec<(PublicKey, Signature)>, // 已收集的投票（公钥和签名）
    used: HashSet<PublicKey>, // 已使用的公钥，用于确保每个公钥只能投票一次
}

impl VotesAggregator {
    // 创建一个新的投票聚合器
    pub fn new() -> Self {
        Self {
            weight: 0, // 初始化时，累计权重为 0
            votes: Vec::new(), // 初始化时，没有任何投票
            used: HashSet::new(), // 初始化时，没有已使用的公钥
        }
    }

    // 添加投票并检查是否达到法定人数
    pub fn append(
        &mut self,
        vote: Vote, // 一个新投票
        committee: &Committee, // 当前委员会配置，用于检查权益
        header: &Header, // 被投票的头部
    ) -> DagResult<Option<Certificate>> {
        let author = vote.author; // 获取投票的作者（公钥）

        // Ensure it is the first time this authority votes.
        // 确保该作者第一次投票
        ensure!(self.used.insert(author), DagError::AuthorityReuse(author));

        // 将该投票添加到集合中
        self.votes.push((author, vote.signature));
        // 增加该投票的权重
        self.weight += committee.stake(&author);
        // 如果累计权重达到或超过法定人数，生成证书
        if self.weight >= committee.quorum_threshold() {
            self.weight = 0; // Ensures quorum is only reached once.
            return Ok(Some(Certificate {
                header: header.clone(), // 克隆头部信息
                votes: self.votes.clone(), // 克隆当前所有投票
            }));
        }
        Ok(None) // 如果未达到法定人数则返回 None
    }
}

/// Aggregate certificates and check if we reach a quorum.
// 聚合证书并检查是否达到法定人数
pub struct CertificatesAggregator {
    weight: Stake,
    certificates: Vec<Certificate>,
    used: HashSet<PublicKey>,
}


impl CertificatesAggregator {
    // 创建一个新的证书聚合器
    pub fn new() -> Self {
        Self {
            weight: 0,
            certificates: Vec::new(),
            used: HashSet::new(),
        }
    }

    pub fn append(
        &mut self,
        certificate: Certificate,
        committee: &Committee,
    ) -> DagResult<Option<Vec<Certificate>>> {
        let origin = certificate.origin();//检查证书来源

        // Ensure it is the first time this authority votes.
        // 确保该来源第一次提交证书
        if !self.used.insert(origin) {
            return Ok(None);
        }

        // 将该证书添加到集合中
        self.certificates.push(certificate);
        self.weight += committee.stake(&origin);
        // 如果累计权重达到或超过法定人数，返回所有证书
        if self.weight >= committee.quorum_threshold() {
            //self.weight = 0; // Ensures quorum is only reached once.
            return Ok(Some(self.certificates.drain(..).collect()));
        }
        Ok(None)
    }
}
