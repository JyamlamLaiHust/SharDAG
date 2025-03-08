# Copyright(C) Facebook, Inc. and its affiliates.
from copy import deepcopy
import subprocess
from os.path import basename, splitext
from time import sleep
import time
from os.path import join
from .commands import CommandMaker
from .config import Key, Committees, LocalCommittee, NodeParameters, BenchParameters, ConfigError
from .logs import LogParser, ParseError
from .utils import Print, BenchError, PathMaker, progress_bar
from collections import OrderedDict

class LocalBench:
    BASE_PORT = 3000

    # 分配端口
    def __init__(self, bench_parameters_dict, node_parameters_dict):
        try:
            # 初始化基准参数和节点参数
            self.bench_parameters = BenchParameters(bench_parameters_dict)
            self.node_parameters = NodeParameters(node_parameters_dict)
            # static local params
            self.client_addr = f"127.0.0.1:{self.BASE_PORT}"
            self.BASE_PORT += 1

        except ConfigError as e:
            raise BenchError('Invalid nodes or bench parameters', e)

    def __getattr__(self, attr):
        # 动态代理，从基准参数中获取属性
        return getattr(self.bench_parameters, attr)

    def _background_run(self, command, log_file):
        # 在后台运行指定命令，并将输出记录到日志文件
        name = splitext(basename(log_file))[0] #提取无后缀的文件名
        cmd = f'{command} 2> {log_file}' #重定向到日志文件
        subprocess.run(['tmux', 'new', '-d', '-s', name, cmd], check=True) #使用tmux后台运行

    def _kill_nodes(self):
        # 终止所有的运行节点
        try:
            cmd = CommandMaker.kill().split()
            subprocess.run(cmd, stderr=subprocess.DEVNULL)
        except subprocess.SubprocessError as e:
            raise BenchError('Failed to kill testbed', e)



    def _config(self, shard_number, shard_size):
        """
        为特定分片数量和大小的配置生成配置文件。
        该配置可以用于多次运行。
        """
        Print.info('Setting up testbed...')  # 输出提示信息
        cmd = CommandMaker.cleanup_config()  # 清理旧的配置
        subprocess.run([cmd], shell=True, stderr=subprocess.DEVNULL)  # 静默执行清理命令

        # 编译最新代码
        cmd = CommandMaker.compile().split()
        subprocess.run(cmd, check=True, cwd=PathMaker.node_crate_path())  # 在指定路径中执行

        # 为客户端和节点生成别名
        cmd = CommandMaker.alias_binaries(PathMaker.binary_path())
        subprocess.run([cmd], shell=True)

        # 初始化配置
        committees = OrderedDict()  # 保证分片顺序一致
        committeeList = []  # 存储每个分片的委员会信息

        print("Node name list: ")
        shardid = 0
        while shardid < shard_number:
            # 为每个分片生成密钥并创建委员会
            keys = []
            key_files = [PathMaker.key_file(i, shardid) for i in range(shard_size)]  # 为分片生成密钥文件路径
            for filename in key_files:
                cmd = CommandMaker.generate_key(filename).split()  # 为每个节点生成密钥
                subprocess.run(cmd, check=True)  # 执行密钥生成命令
                keys += [Key.from_file(filename)]  # 从文件中加载密钥

            names = [x.name for x in keys]  # 获取节点名称
            print("shard", shardid, sorted(names))  # 输出分片信息
            # 创建本地委员会对象
            committee = LocalCommittee(names, self.BASE_PORT, self.workers, shardid)
            committee.print(PathMaker.committee_file(shardid))  # 输出委员会信息到文件
            committeeList.append(committee)  # 保存委员会到列表
            committees[shardid] = committee.json  # 将委员会转换为JSON格式并存储

            shardid += 1
            self.BASE_PORT += 6 * shard_size  # 更新端口以适配更多节点

        # 生成节点参数文件
        self.node_parameters.print(PathMaker.parameters_file())
        # 生成委员会配置文件
        committees = Committees(committees, self.client_addr, shard_number, shard_size)
        committees.print(PathMaker.committees_file())

        return committeeList  # 返回委员会列表

    def _run_single(self, executor_type, epoch, shard_num, shard_size, total_rate, total_txs, committeeList, debug=False):

      # Kill any previous testbed.
      self._kill_nodes()
      # Clean .log and .db file
      cmd = f'{CommandMaker.clean_logs()} ; {CommandMaker.clean_dbs()}'
      subprocess.run([cmd], shell=True, stderr=subprocess.DEVNULL)
      sleep(0.5)  # Removing the db store may take time.

      # Run committees
      all_running_worker_addrs = []
      shardid = 0
      while shardid < shard_num:        
          workers_addresses = committeeList[shardid].workers_addresses(self.faults)   

          # Run the primaries (except the faulty ones).
          for nodeid, address in enumerate(committeeList[shardid].primary_addresses(self.faults)):
              cmd = CommandMaker.run_primary(
                  PathMaker.parameters_file(),
                  PathMaker.committees_file(),
                  shardid,
                  PathMaker.key_file(nodeid, shardid),
                  PathMaker.db_path(nodeid, shardid),
                  debug
              )
              log_file = PathMaker.primary_log_file(nodeid, shardid)
              self._background_run(cmd, log_file)

          # Run the workers (except the faulty ones).
          for nodeid, addresses in enumerate(workers_addresses):
              is_cs_fault = 0
              if nodeid < self.bench_parameters.cs_faults:
                is_cs_fault = 1
              for (id, address) in addresses: # run each node
                  all_running_worker_addrs.append(address)
                  cmd = CommandMaker.run_worker(
                      executor_type,
                      self.bench_parameters.state_store_type,
                      self.bench_parameters.acc_shard_type,
                      self.bench_parameters.append_type,
                      PathMaker.parameters_file(),
                      PathMaker.committees_file(),
                      shardid,
                      id,  # worker's id.
                      self.bench_parameters.cs_faults,
                      is_cs_fault,
                      PathMaker.key_file(nodeid, shardid),
                      PathMaker.db_path(nodeid, shardid, 0),
                      PathMaker.ft_db_path(nodeid, shardid),
                      # fot testing tps & lantecy
                      PathMaker.acc2shard_file_default(),
                      PathMaker.acc2shard_file_default(),
                      # PathMaker.acc2shard_file(epoch, shard_num),
                      # PathMaker.actacc2shard_file(epoch, shard_num),
                      epoch,
                      debug,
                  )
                  # print(cmd)
                  log_file = PathMaker.worker_log_file(nodeid, id, shardid)
                  self._background_run(cmd, log_file)
          shardid += 1

      # Run the client (it will wait for all nodes to be ready).
      cmd = CommandMaker.run_client(
          executor_type,
          self.bench_parameters.acc_shard_type,
          PathMaker.committees_file(),   
          self.client_addr,
          self.tx_size,
          total_rate,
          total_txs,
          PathMaker.workload_input_file(epoch),
          PathMaker.acc2shard_file(epoch, shard_num),
          PathMaker().brokers_file(),
          all_running_worker_addrs,
          epoch,
      )
      log_file = PathMaker.client_log_file()
      self._background_run(cmd, log_file)

      # Wait for all transactions to be processed.
      # Print.info(f'Running benchmark ({self.duration} sec)...')
      print(time.strftime('%Y-%m-%d %H:%M:%S',time.localtime(time.time())), "Waiting...")
      for _ in progress_bar(range(self.duration), prefix=f'Running benchmark ({self.duration} sec):'):
          sleep(1) 
      print(time.strftime('%Y-%m-%d %H:%M:%S',time.localtime(time.time())), "Done!")
      self._kill_nodes()



    def _get_storage_cost(self, cmd_MB):
        storage_MB = 0
        p = subprocess.Popen(cmd_MB, shell=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        for line in p.stdout.readlines():
            tmp = int(line.decode().split('\t')[0])
            storage_MB += tmp
        return storage_MB

    def measure_storage_cost(self, shard_number, shard_size):
      ## storage cost: mean_max
      res_ledger_MB = []
      res_state_MB = []
      for shardid in range(shard_number):
        res_ledger_MB_tmp = []
        res_store_MB_tmp = []
        for nodeid in range(shard_size):
          # 1. get ledger storage cost of node
          db_path = join(PathMaker.dbs_path(), f'.db-*-{nodeid}-{shardid}')
          cmd_MB = f'du --block-size=1M --max-depth=1 {db_path}'

          ledger_MB = self._get_storage_cost(cmd_MB)
          # print(cmd_MB, shardid, nodeid, ledger_MB)
          res_ledger_MB_tmp.append(ledger_MB)

          # 2. get state storage cost of node
          db_path = join(PathMaker.dbs_path(), f'.db-{nodeid}-{shardid}-*')
          cmd_MB = f'du --block-size=1M --max-depth=1 {db_path}'
          state_MB = self._get_storage_cost(cmd_MB)
          # print(cmd_MB, shardid, nodeid, state_MB)
          res_store_MB_tmp.append(state_MB)

        res_ledger_MB.append(max(res_ledger_MB_tmp))
        res_state_MB.append(max(res_store_MB_tmp))

      # print(res_ledger_MB)
      # print(res_state_MB)
      return res_ledger_MB, res_state_MB


    def run(self, debug=False):
        # 运行完整的基准测试
        assert isinstance(debug, bool) # 确保debug是布尔值
        Print.heading('Starting local benchmark')

        for shard_number in self.bench_parameters.shard_numbers: 
          for shard_size in self.bench_parameters.nodes:
            # generate config_file for (shard_number, shard_size) setting
            Print.heading(f'\n-Configurating {shard_number} shards, {shard_size} nodes')
            committeeList = self._config(shard_number, shard_size)
            
            for r in self.bench_parameters.rate:
                Print.heading(f'\n--Running {shard_size} nodes, {shard_number} shards (input rate: {r:,} tx/s)')   

                total_rate = r * shard_size * shard_number
                
                for executor_type in self.bench_parameters.executor_type: # baseline
                  Print.heading(f'\n--Executor_type: {executor_type}')
                    
                  # set append_type according to executor_type
                  if self.bench_parameters.cs_faults != 0:
                    if executor_type == 0: # SharDAG
                      self.bench_parameters.append_type = 0 # Dual_Mode
                    else:
                      self.bench_parameters.append_type = 1 # Serial
                  else:
                    self.bench_parameters.append_type = 1 # Serial


                  for e in range(self.bench_parameters.runs[0]): # repeat `runs` times
                    Print.heading(f'Run {e+1}/{self.bench_parameters.runs}')
                  # for e in self.bench_parameters.runs: # run epoch e
                    # Print.heading(f'----Run epoch {e}. {self.bench_parameters.runs}')
                    try:
                      self._run_single(executor_type, e, shard_number, shard_size, total_rate, self.bench_parameters.total_txs, committeeList, debug)

                      # Measure storage cost
                      res_ledger_MB, res_state_MB = self.measure_storage_cost(shard_number, shard_size)

                      # Parse logs and return the parser.
                      print(time.strftime('%Y-%m-%d %H:%M:%S',time.localtime(time.time())), "Parsing logs...")
                      # Print.info('Parsing logs...')
                      logger = LogParser.process(PathMaker.logs_path(), e, shard_number, self.faults, self.bench_parameters.cs_faults, self.bench_parameters.total_txs, self.duration, self.sample_interval, res_ledger_MB, res_state_MB)
                      logger.print(PathMaker.result_file(
                          "local",
                          self.bench_parameters.acc_shard_type,
                          executor_type,
                          self.bench_parameters.state_store_type,
                          shard_number,
                          shard_size,
                          self.faults,
                          self.bench_parameters.cs_faults,
                          r, 
                          self.bench_parameters.duration,
                          self.bench_parameters.total_txs,
                          self.node_parameters.batch_size,
                      ))

                    except (subprocess.SubprocessError, ParseError) as e:
                        self._kill_nodes()
                        raise BenchError('Failed to run benchmark', e)
                    continue
