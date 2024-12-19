// 导入外部库
use std::convert::TryFrom;
use anyhow::{Context, Result};
use clap::{crate_name, crate_version, App, AppSettings, ArgMatches, SubCommand};

// 导入自定义库
use config::Export as _;
use config::Import as _;
use config::{Committees, KeyPair, Parameters, WorkerId, ShardId};
use consensus::Consensus;
use env_logger::Env;
use log::info;
use primary::{Certificate, Primary};
use store::Store;
use tokio::sync::mpsc::{channel, Receiver};
use worker::Account2Shard;
use worker::Account2ShardGraph;
use worker::Account2ShardHash;
use worker::Account2ShardType;
use worker::AppendType;
use worker::ExecutorType;
use worker::StateStoreType;
use worker::new_primary_store;
use worker::Worker;

// 导入自定义模块
mod benchmark_client;
mod test_migration;
mod test_execution;

/// 定义默认通道容量，用于限制异步消息通道的队列大小
pub const CHANNEL_CAPACITY: usize = 1_000;

// 使用 tokio 的异步入口，当出现错误时主函数返回 Result 类型处理潜在错误
#[tokio::main]
async fn main() -> Result<()> {
    //
    let matches = App::new(crate_name!())
        .version(crate_version!())
        .about("A research implementation of Narwhal and Tusk.")
        .args_from_usage("-v... 'Sets the level of verbosity'")
        .subcommand(
            // 生成新的密钥对并输出到指定文件
            SubCommand::with_name("generate_keys")
                .about("Print a fresh key pair to file")
                .args_from_usage("--filename=<FILE> 'The file where to print the new key pair'"),
        )
        .subcommand(
            // 启动一个节点，分为 primary 和 worker 两种模式
            SubCommand::with_name("run")
                .about("Run a node")
                .args_from_usage("--keys=<FILE> 'The file containing the node keys'")
                .args_from_usage("--committee=<FILE> 'The file containing committee information'")
                .args_from_usage("--parameters=[FILE] 'The file containing the node parameters'")
                .args_from_usage("--store=<PATH> 'The path where to create the data store'")
                .args_from_usage("--shardid=<INT> 'The shard id'")
                .subcommand(SubCommand::with_name("primary").about("Run a single primary"))
                .subcommand(
                    SubCommand::with_name("worker")
                        .about("Run a single worker")
                        .args_from_usage("--id=<INT> 'The worker id'")
                        .args_from_usage("--cs_faults=<INT> 'cs_faults'")
                        .args_from_usage("--is_cs_fault=<INT> 'is_cs_fault'")
                        .args_from_usage("--acc2shard=[FILE] 'The file containing the acc2shard map'")
                        .args_from_usage("--actacc2shard=[FILE] 'The file containing the actacc2shard map'")
                        .args_from_usage("--ftstore=<PATH> 'The path where to create the full t store'")
                        .args_from_usage("--state_store_type=<INT> 'state_store_type'")
                        .args_from_usage("--executor_type=<INT> 'executor_type'")
                        .args_from_usage("--acc_shard_type=<INT> 'acc_shard_type'")
                        .args_from_usage("--append_type=<INT> 'append_type'")
                        .args_from_usage("--epoch=<INT> 'The current epoch'")
                )
                .setting(AppSettings::SubcommandRequiredElseHelp),
        )
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .get_matches();

    // 根据命令行的 -v 设置日志级别
    let log_level = match matches.occurrences_of("v") {
        0 => "error",
        1 => "warn",
        2 => "info",
        3 => "debug",
        _ => "trace",
    };
    // 初始化时间戳
    let mut logger = env_logger::Builder::from_env(Env::default().default_filter_or(log_level));
    #[cfg(feature = "benchmark")]
    logger.format_timestamp_millis();
    logger.init();

    // 处理子命令
    match matches.subcommand() {
        // 创建密钥对并保存到文件中
        ("generate_keys", Some(sub_matches)) => KeyPair::new()
            .export(sub_matches.value_of("filename").unwrap())
            .context("Failed to generate key pair")?,
        // 调用 run 函数，启动节点逻辑
        ("run", Some(sub_matches)) => run(sub_matches).await?,
        _ => unreachable!(),
    }
    Ok(())
}

// run 函数细节
// Runs either a worker or a primary.
async fn run(matches: &ArgMatches<'_>) -> Result<()> {
    // 读取密钥文件、委员会信息和存储路径
    let key_file = matches.value_of("keys").unwrap();
    let committee_file = matches.value_of("committee").unwrap();
    let parameters_file = matches.value_of("parameters");
    let store_path = matches.value_of("store").unwrap();

    // 解析分片编号
    let shard_id = matches.value_of("shardid").unwrap()
                .parse::<ShardId>()
                .context("The shard id must be a positive integer")?;

    // 加载密钥和委员会信息
    // Read the node's keypair from file.
    let keypair = KeyPair::import(key_file).context("Failed to load the node's keypair")?; 

    //Read all committees from file.
    let committees =
        Committees::import(committee_file).context("Failed to load the committees information")?;
    let our_committee = 
        committees.our_committee(&shard_id).context("Failed to load the committee information")?;
    
    let shard_num = committees.shard_num();

    // Load default parameters if none are specified.
    let parameters = match parameters_file {
        Some(filename) => {
            Parameters::import(filename).context("Failed to load the node's parameters")?
        }
        None => Parameters::default(),
    };

    // Make the data store.
    let store = Store::new(store_path).context("Failed to create a store")?;

    // Channels the sequence of certificates.
    let (tx_output, rx_output) = channel(CHANNEL_CAPACITY);

    // Check whether to run a primary, a worker, or an entire authority.
    match matches.subcommand() {
        // 启动主节点和共识模块
        // Spawn the primary and consensus core.
        ("primary", _) => {
            let (tx_new_certificates, rx_new_certificates) = channel(CHANNEL_CAPACITY);
            let (tx_feedback, rx_feedback) = channel(CHANNEL_CAPACITY);
            Primary::spawn(
                keypair,
                our_committee.clone(),
                parameters.clone(),
                store,
                /* tx_consensus */ tx_new_certificates,
                /* rx_consensus */ rx_feedback,
            );
            Consensus::spawn(
                our_committee,
                parameters.gc_depth,
                /* rx_primary */ rx_new_certificates,
                /* tx_primary */ tx_feedback,
                tx_output,
            );
        }

        // 启动工作节点，负责分片相关任务
        // Spawn a single worker.
        ("worker", Some(sub_matches)) => {
            let id = sub_matches
                .value_of("id")
                .unwrap()
                .parse::<WorkerId>()
                .context("The worker id must be a positive integer")?;

            let cs_faults = sub_matches
                .value_of("cs_faults")
                .unwrap()
                .parse::<usize>()
                .context("cs_faults")?;
            info!("cs_faults: {}", cs_faults);
            let is_cs_fault = sub_matches
                .value_of("is_cs_fault")
                .unwrap()
                .parse::<u8>()
                .context("is cs fault")?;
            info!("is_cs_fault: {}", is_cs_fault);
            let mut is_malicious = false;
            if is_cs_fault == 1 {
              is_malicious = true;
            }


            let epoch = sub_matches
                .value_of("epoch")
                .unwrap()
                .parse::<usize>()
                .context("epoch must be a positive integer")?;

            info!("Epoch: {:?}", epoch);

            let full_store_path = sub_matches.value_of("ftstore").unwrap();

            let acc2shard_file = sub_matches.value_of("acc2shard").unwrap();// csv
            info!("acc2shard_file: {:?}", acc2shard_file);
            let actacc2shard_file = sub_matches.value_of("actacc2shard").unwrap();// csv
            info!("actacc2shard_file: {:?}", actacc2shard_file);

            // load test method
            let acc_shard_type = sub_matches
                .value_of("acc_shard_type")
                .unwrap()
                .parse::<usize>()
                .context("acc_shard_type must be a positive integer")?;  
            let acc_shard_type = Account2ShardType::try_from(acc_shard_type).unwrap();  
            info!("acc_shard_type: {:?}", acc_shard_type);
            let executor_type = sub_matches
                .value_of("executor_type")
                .unwrap()
                .parse::<usize>()
                .context("executor_type must be a positive integer")?;
            let executor_type = ExecutorType::try_from(executor_type).unwrap();
            info!("executor_type: {:?}", executor_type);
            let tmp = sub_matches
              .value_of("state_store_type")
              .unwrap()
              .parse::<usize>()
              .context("state_store_type must be a positive integer")?;
            let state_store_type: StateStoreType;
            match executor_type {
              ExecutorType::SharDAG => {
                state_store_type = StateStoreType::try_from(tmp).unwrap();  
              },
              _ => {
                state_store_type = StateStoreType::MStore;
              }
            }
            info!("state_store_type: {:?}", state_store_type);

            let append_type = sub_matches
                .value_of("append_type")
                .unwrap()
                .parse::<usize>()
                .context("append_type must be a positive integer")?;  
            let append_type = AppendType::try_from(append_type).unwrap();  
            info!("append_type: {:?} ", append_type);


            let acc2shard: Box<dyn Account2Shard + Send>;
            match acc_shard_type {
              Account2ShardType::HashPolicy => {
                acc2shard = Box::new(Account2ShardHash::new(shard_num));
              },
              Account2ShardType::GraphPolicy => {
                acc2shard = Box::new(Account2ShardGraph::new(shard_num, &acc2shard_file));
              }
            }
            // initialize local state store
            let primary_store = new_primary_store(
              shard_id, acc2shard_file, actacc2shard_file, &acc2shard, state_store_type, full_store_path,
            ).await;
      
            Worker::spawn(executor_type, append_type, keypair.name,keypair.secret, id, cs_faults, is_malicious, our_committee, parameters, store, shard_id, committees, primary_store, acc2shard);
        }
        _ => unreachable!(),
    } 

    // 分析共识输出
    // Analyze the consensus' output.
    analyze(rx_output).await;

    // If this expression is reached, the program ends and all other tasks terminate.
    unreachable!();
}

/// Receives an ordered list of certificates and apply any application-specific logic.
async fn analyze(mut rx_output: Receiver<Certificate>) {
    while let Some(_certificate) = rx_output.recv().await {
        // NOTE: Here goes the application logic.
    }
}