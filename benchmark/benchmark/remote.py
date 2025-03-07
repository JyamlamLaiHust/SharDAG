# Copyright(C) Facebook, Inc. and its affiliates.
from collections import OrderedDict
import os
import time
from fabric import Connection, ThreadingGroup as Group # type: ignore
from fabric.exceptions import GroupException # type: ignore
from os.path import basename, splitext
from time import sleep
import subprocess
import math
from os.path import join
from .config import Committee, Committees, Key, NodeParameters, BenchParameters, ConfigError
from .utils import BenchError, Print, PathMaker, progress_bar
from .commands import CommandMaker
from .logs import LogParser, ParseError


class FabricError(Exception):
    ''' Wrapper for Fabric exception with a meaningfull error message. '''

    def __init__(self, error):
        assert isinstance(error, GroupException)
        message = list(error.result.values())[-1]
        super().__init__(message)


class ExecutionError(Exception):
    pass


class RemoteBench:
    def __init__(self, ctx):

        self.repo_name = "SharDAG"
        self.repo_url = "https://github.com/JyamlamLaiHust/SharDAG.git"
        self.repo_branch = "main"

        # remote hosts
        self.user='root'
        self.connect = {"password": 'SharDAG123'}
        self.base_port = 5000
        self.nodes_per_host = 10
        self.hosts = [
            # 每天开机都要检查 ip

            '47.243.168.208:22',
            '47.243.165.254:22',
            '47.243.171.254:22',
            '47.243.168.147:22',
          ]
        self.ip_to_host = { host.split(':')[0]:host for host in self.hosts}
        self.client_addr = f"47.243.109.254:5000"


    def _check_stderr(self, output):
        if isinstance(output, dict):
            for x in output.values():
                if x.stderr:
                    raise ExecutionError(x.stderr)
        else:
            if output.stderr:
                raise ExecutionError(output.stderr)

    def test(self):
        Print.info('test...')          
        try:
            g = Group(*self.hosts, user='root', connect_kwargs=self.connect)
            cmd = 'ls'
            g.run(f'{cmd} || true', hide=True)
        except (GroupException, ExecutionError) as e:
            e = FabricError(e) if isinstance(e, GroupException) else e
            raise BenchError('Failed to run test', e)


    def findAllFile(self, base):
        for root, ds, fs in os.walk(base):
            for f in fs:
                yield f

    def unload_input_files(self):
        # hosts = [
        #       '10.176.50.16:32772',
        # ]
        Print.info('Upload input files ...')
        try:
            cmd = 'mkdir input'
            g = Group(*self.hosts, user='root', connect_kwargs=self.connect)

            # total = 0
            # base = '../../inputv2'
            # for i in self.findAllFile(base):
            #   print(i)
            #   g.put(f'../../inputv2/{i}', './SharDAG-Workspace/inputv2')
            #   total += 1
            # print(total)
            
            # cmd = 'apt-get install unzip ; rm -r acc-input ; unzip acc-input.zip'
            g.run(f'{cmd} || true', hide=True)
            # g.put('/home/jaylen/SharDAG/test-workload/input-e0.csv', './input')
            g.put('/root/SharDAG/test-workload/input-e0.csv', './input')
        except (GroupException, ExecutionError) as e:
            e = FabricError(e) if isinstance(e, GroupException) else e
            raise BenchError('Failed to upload input files', e)        


    def install(self):
        Print.info('Installing rust...')
        cmd = [
          # 'sudo rm /var/lib/dpkg/lock-frontend',
          # 'sudo rm /var/lib/dpkg/lock',
          # 'sudo rm /var/cache/apt/archives/lock',
          # 'sudo dpkg --configure -a',
          #
          'export RUSTUP_DIST_SERVER=https://mirrors.ustc.edu.cn/rust-static',
          'export RUSTUP_UPDATE_ROOT=https://mirrors.ustc.edu.cn/rust-static/rustup',

          'apt-get update',
          'apt-get -y upgrade',
          'apt-get -y autoremove',

          # The following dependencies prevent the error: [error: linker `cc` not found].
          'apt-get -y install build-essential',
          'apt-get -y install cmake',

          # Install rust (non-interactive).
          'curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y',
          'source $HOME/.cargo/env',
          'rustup default stable',

          # This is missing from the Rocksdb installer (needed for Rocksdb).
          'apt-get install -y clang',

          'apt-get install -y tmux',
          'apt-get install -y git',
          'sudo rm -rf SharDAG',
        ]
        
        hosts = self.hosts # get node ips
        print("all hosts: ", hosts)
        try:
            g = Group(*hosts, user='root', connect_kwargs=self.connect)
            print("test")
            g.run(' && '.join(cmd), hide=False)
            print("end")
            Print.heading(f'Initialized testbed of {len(hosts)} nodes')
        except (GroupException, ExecutionError) as e:
            e = FabricError(e) if isinstance(e, GroupException) else e
            raise BenchError('Failed to install rust on testbed', e)


    def install_repo(self):
        Print.info('Cloning the repo...')

        cmd = [
            # Clone the repo.
            # 'sudo rm -rf SharDAG',
            f'(git clone -b {self.repo_branch} {self.repo_url})'
        ]
        hosts = self.hosts # get node ips
        print("all hosts: ", hosts)
        try:
            g = Group(*hosts, user='root', connect_kwargs=self.connect)
            g.run(' && '.join(cmd), hide=False)
            Print.heading(f'Cloned repo of {len(hosts)} nodes')
        except (GroupException, ExecutionError) as e:
            e = FabricError(e) if isinstance(e, GroupException) else e
            raise BenchError('Failed to install repo on testbed', e)


    def kill(self, hosts=[], delete_logs=False, delete_dbs=False):
        assert isinstance(hosts, list)
        assert isinstance(delete_logs, bool)
        assert isinstance(delete_dbs, bool)
        # print("kill: ", hosts)
        delete_logs = CommandMaker.clean_logs() if delete_logs else 'true'
        delete_dbs = CommandMaker.clean_dbs() if delete_dbs else 'true'
        cmd = [f'({CommandMaker.kill()} || true)']
        try:
            g = Group(*hosts,  user='root', connect_kwargs=self.connect)
            g.run(' && '.join(cmd), hide=True)
            g.run(delete_logs, hide=True)
            g.run(delete_dbs, hide=True)
        except GroupException as e:
            raise BenchError('Failed to kill nodes', FabricError(e))
        # kill client
        try:
            cmd = CommandMaker.kill().split()
            subprocess.run(cmd, stderr=subprocess.DEVNULL)
        except subprocess.SubprocessError as e:
            raise BenchError('Failed to kill client', e)

    def _background_run_client(self, command, log_file):
        name = splitext(basename(log_file))[0]
        cmd = f'{command} 2> {log_file}'
        subprocess.run(['tmux', 'new', '-d', '-s', name, cmd], check=True)


    def _background_run(self, host, command, log_file):
        name = splitext(basename(log_file))[0]
        cmd = f'tmux new -d -s "{name}" "{command} |& tee {log_file}"'
        c = Connection(host,  user='root', connect_kwargs=self.connect)
        output = c.run(cmd, hide=True)
        self._check_stderr(output)

    def complie(self, remote_recompile):
      # Recompile the latest code.
      cmd = CommandMaker.compile().split()
      subprocess.run(cmd, check=True, cwd=PathMaker.node_crate_path())

      # Create alias for the client and nodes binary.
      cmd = CommandMaker.alias_binaries(PathMaker.binary_path())
      subprocess.run([cmd], shell=True)

      # Update nodes.
      if remote_recompile:
        self.remote_update_compile(self.hosts, self.bench_parameters.collocate)

    def remote_update_compile(self, hosts, collocate): # selected hosts
        if collocate:
            ips = list(set(hosts))
        else:
            ips = list(set([x for y in hosts for x in y]))

        Print.info(
            f'Updating {len(ips)} machines (branch "{self.repo_branch}")...'
        )
        cmd = [
            f'(cd {self.repo_name} && git fetch -f)',
            f'(cd {self.repo_name} && git checkout -f {self.repo_branch})',
            f'(cd {self.repo_name} && git pull -f)',
            'source $HOME/.cargo/env',
            f'(cd {self.repo_name}/node && {CommandMaker.compile()})',
            CommandMaker.alias_binaries(
                f'./{self.repo_name}/target/release/'
            )
        ]
        try: 
          g = Group(*ips,  user='root', connect_kwargs=self.connect)
          g.run(' && '.join(cmd), hide=False)
        except (GroupException, ExecutionError) as e:
            e = FabricError(e) if isinstance(e, GroupException) else e
            raise BenchError('Failed to update nodes', e)


    def get_hosts_for_shard(self, used_host_ips, shardId, shard_size):  
      hosts = []
      for nodeId in range(shard_size):
        next_host_index = shardId * shard_size + nodeId
        hosts.append(used_host_ips[next_host_index % len(used_host_ips)])
      return hosts

    
    def _genconfig_and_upload(self, ips, base_port_for_used_hosts, shard_number, shard_size, node_parameters, bench_parameters):
        Print.info('Generating configuration files...')

        # Cleanup all local configuration files.
        cmd = CommandMaker.cleanup_config()
        subprocess.run([cmd], shell=True, stderr=subprocess.DEVNULL)

        # Generate configuration files.
        committees = OrderedDict()
        committeeList = []
        
        # generate configuration files for shard temp
        for shardid in range(shard_number):
            keys = []
            key_files = [PathMaker.key_file(i, shardid) for i in range(shard_size)]
            for filename in key_files:
                cmd = CommandMaker.generate_key(filename).split() 
                subprocess.run(cmd, check=True)
                keys += [Key.from_file(filename)]
                # print(keys)  # for debug
            
            names = [x.name for x in keys]
            print("shard ", shardid, sorted(names))
            # get all ips for nodes in shard `shardid`
            temp_hosts = self.get_hosts_for_shard(ips, shardid, shard_size)
            temp_ips = [host.split(':')[0] for host in temp_hosts]
            print(f"node ips in shard {shardid}: {temp_ips}")

            # generate ip_list for each node in shard shardid
            if bench_parameters.collocate:
                workers = bench_parameters.workers
                addresses = OrderedDict(
                    (x, [y] * (workers + 1)) for x, y in zip(names, temp_ips) # x: name, y: a host
                )
            else:
                addresses = OrderedDict(
                    (x, y) for x, y in zip(names, temp_ips)
                )  
            # print(f"addresses in shard {shardid}: {addresses}")

            # get base port for each node in shard shardid
            base_port_of_each_node = []
            for host_ip in temp_ips:
              port = base_port_for_used_hosts[host_ip]
              base_port_for_used_hosts[host_ip] += 6 # a node needs 6 ports
              base_port_of_each_node.append(port)


            print(f"base port of nodes in shard {shardid}: {base_port_of_each_node}")
            #  `addresses`` contains the addrs of all nodes in a committee：(name, [primary_ip, worker_ip]) 
            committee = Committee(addresses, base_port_of_each_node, shardid)             
            committee.print(PathMaker.committee_file(shardid))
            committeeList.append(committee)
            committees[shardid] = committee.json       
        
        # generate .parameters.json
        node_parameters.print(PathMaker.parameters_file())
        # generate .committees.json
        committees = Committees(committees, self.client_addr, shard_number, shard_size)
        committees.print(PathMaker.committees_file())


        # Cleanup all nodes and upload configuration files.
        # each host need `committees_file` and `parameter_file`
        hosts = [self.ip_to_host[host_ip] for host_ip in ips] #  `ips` contains the ip of all hosts
        print("Upload config files to hosts: ", hosts)
        try:
            g = Group(*hosts, user='root', connect_kwargs=self.connect)
            g.run(f'{CommandMaker.cleanup_config()} || true', hide=True)
            g.put(PathMaker.committees_file(), PathMaker.configs_path())
            g.put(PathMaker.parameters_file(), PathMaker.configs_path())
        except (GroupException, ExecutionError) as e:
            e = FabricError(e) if isinstance(e, GroupException) else e
            raise BenchError('Failed to upload config file on testbed', e)

        # upload key_file for each node
        progress = progress_bar(committeeList, prefix=f'Uploading key files for each node:')
        for shardid, committee in enumerate(progress):
          for nodeid, address in enumerate(committee.primary_addresses()):
              host_ip = Committee.ip(address)
              c = Connection(self.ip_to_host[host_ip],  user='root', connect_kwargs=self.connect)
              c.put(PathMaker.key_file(nodeid, shardid), PathMaker.configs_path())

        return committeeList

    def _run_single(self, used_hosts, executor_type, epoch, shard_num, shard_size, total_rate, total_txs, committeeList, debug=False):
        # Delete local logs (if any).
        cmd = CommandMaker.clean_logs()
        subprocess.run([cmd], shell=True, stderr=subprocess.DEVNULL)

        # Kill any potentially unfinished run and delete logs.
        self.kill(hosts=used_hosts, delete_logs=True, delete_dbs=True)

        all_running_worker_addrs = []
        # Run committees
        print(time.strftime('%Y-%m-%d %H:%M:%S',time.localtime(time.time())), "Booting nodes...")
        progress = progress_bar(committeeList, prefix=f'Booting all shards:')
        for shardid, committee in enumerate(progress):
          workers_addresses = committee.workers_addresses(self.bench_parameters.faults)

          # Run the primaries (except the faulty ones).
          # Print.info(f'Booting primaries for shard {shardid}...')
          for nodeid, address in enumerate(committee.primary_addresses(self.bench_parameters.faults)):
              host = Committee.ip(address)
              host = self.ip_to_host[host]
              cmd = CommandMaker.run_primary(
                  PathMaker.parameters_file(),
                  PathMaker.committees_file(),
                  shardid,
                  PathMaker.key_file(nodeid, shardid),
                  PathMaker.db_path(nodeid, shardid),
                  debug
              )
              log_file = PathMaker.primary_log_file(nodeid, shardid)
              # print("run primary: ", address, "host: ", host, cmd, log_file)
              self._background_run(host, cmd, log_file)

          # Run the workers (except the faulty ones).
          # Print.info(f'Booting workers for shard {shardid}...')
          for nodeid, addresses in enumerate(workers_addresses):
              is_cs_fault = 0
              if nodeid < self.bench_parameters.cs_faults:
                is_cs_fault = 1
              for (id, address) in addresses: # run each node
                  all_running_worker_addrs.append(address)
                  host_ip = Committee.ip(address)
                  host = self.ip_to_host[host_ip]
                  cmd = CommandMaker.run_worker(
                      executor_type,
                      self.bench_parameters.state_store_type,
                      self.bench_parameters.acc_shard_type,
                      self.bench_parameters.append_type,
                      PathMaker.parameters_file(),
                      PathMaker.committees_file(),
                      shardid,
                      id,  # The worker's id.
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
                      self.bench_parameters.sample_interval,
                      debug,
                  )
                  log_file = PathMaker.worker_log_file(nodeid, id, shardid)
                  # print("run worker: ", address, "host: ", host, cmd, log_file)
                  self._background_run(host, cmd, log_file)

        # Run the client (it will wait for all nodes to be ready).
        cmd = CommandMaker.run_client(
          executor_type,
          self.bench_parameters.acc_shard_type,
          PathMaker.committees_file(),   
          self.client_addr,
          self.bench_parameters.tx_size,
          total_rate,
          total_txs,
          PathMaker.workload_input_file(epoch),
          PathMaker.acc2shard_file(epoch, shard_num),
          PathMaker().brokers_file(),
          all_running_worker_addrs,
          epoch,
        )
        log_file = PathMaker.client_log_file()
        self._background_run_client(cmd, log_file)

        # Wait for all transactions to be processed.
        duration = self.bench_parameters.duration
        print(time.strftime('%Y-%m-%d %H:%M:%S',time.localtime(time.time())), "Running benchmark...")
        for _ in progress_bar(range(duration), prefix=f'Running benchmark ({duration} sec):'):
            sleep(1)            
        print(time.strftime('%Y-%m-%d %H:%M:%S',time.localtime(time.time())), "Done...\n")
        self.kill(hosts=used_hosts, delete_logs=False, delete_dbs=False)


    def _logs(self, committeeList, epoch, shard_number, faults, duration, sample_interval):
        res_ledger_MB = []
        res_state_MB = []

        # Download log files.
        # progress = progress_bar(committeeList, prefix=f'Download log files:')
        print("Download log files...")
        # for shardid, committee in enumerate(progress):
        for shardid, committee in enumerate(committeeList):
          res_ledger_MB_tmp = []
          res_store_MB_tmp = []
          workers_addresses = committee.workers_addresses(faults)
          # progress = progress_bar(workers_addresses, prefix=f'Downloading workers logs of shard {shardid}:')
          print(f'Downloading workers logs of shard {shardid}:')
          for nodeid, addresses in enumerate(workers_addresses):
          # for nodeid, addresses in enumerate(progress): 
              for id, address in addresses: # worker 0
                  host = Committee.ip(address)
                  host = self.ip_to_host[host]
                  c = Connection(host, user='root', connect_kwargs=self.connect)
                  c.get(
                      PathMaker.worker_log_file(nodeid, id, shardid), 
                      local=PathMaker.worker_log_file(nodeid, id, shardid)
                  )

                  # measure storage cost 
                  # 1. get ledger storage cost of node
                  db_path = join(PathMaker.dbs_path(), f'.db-*-{nodeid}-{shardid}')
                  cmd_MB = f'du --block-size=1M --max-depth=1 {db_path}'
                  ledger_MB = self._get_storage_cost(c, cmd_MB)
                  print(cmd_MB, shardid, nodeid, ledger_MB)
                  res_ledger_MB_tmp.append(ledger_MB)

                  # 2. get state storage cost of node
                  db_path = join(PathMaker.dbs_path(), f'.db-{nodeid}-{shardid}-*')
                  cmd_MB = f'du --block-size=1M --max-depth=1 {db_path}'
                  state_MB = self._get_storage_cost(c, cmd_MB)
                  print(cmd_MB, shardid, nodeid, state_MB)
                  res_store_MB_tmp.append(state_MB)
          res_ledger_MB.append(max(res_ledger_MB_tmp)) # choose the max_storage_cost in each shard
          res_state_MB.append(max(res_store_MB_tmp))
        
          ## download primaries logs...
          primary_addresses = committee.primary_addresses(faults)
          progress = progress_bar(primary_addresses, prefix=f'Downloading primaries logs of shard {shardid}:')
          for nodeid, address in enumerate(progress):
              host = Committee.ip(address)
              host = self.ip_to_host[host]
              c = Connection(host, user='root', connect_kwargs=self.connect)
              c.get(
                  PathMaker.primary_log_file(nodeid, shardid), 
                  local=PathMaker.primary_log_file(nodeid, shardid)
              )
        ## end of for shardid

        # Parse logs and return the parser.
        Print.info('Parsing logs and computing performance...')
        return LogParser.process(PathMaker.logs_path(), epoch, shard_number, faults, self.bench_parameters.cs_faults, self.bench_parameters.total_txs, duration, sample_interval, res_ledger_MB, res_state_MB)


    def _get_storage_cost(self, c, cmd_MB):
        storage_MB = 0
        res = c.run(cmd_MB, hide=True) # res.stdout is a str
        lines = res.stdout.split('\n')
        for line in lines:
          if line != '':
            tmp = line.split('\t')[0]
            storage_MB += int(tmp)
        return storage_MB


    def run(self, bench_parameters_dict, node_parameters_dict, debug=False, remote_recompile=False):
        # parameters
        try:
            self.bench_parameters = BenchParameters(bench_parameters_dict)
            self.node_parameters = NodeParameters(node_parameters_dict)
        except ConfigError as e:
            raise BenchError('Invalid nodes or bench parameters', e)


        Print.heading('Starting remote benchmark')
        # complie the latest binary file
        self.complie(remote_recompile)

        for shard_number in self.bench_parameters.shard_numbers:
          for shard_size in self.bench_parameters.nodes:
            
              # # TODO just for measuring appending delay. set cs_faults according to shard_size
              # self.bench_parameters.cs_faults = int((shard_size - 1) / 3)
              
              Print.heading(f'\nShard num: {shard_number}, Shard size: {shard_size} ...')      
              # choose number of hosts used in this parameter setting
              faults = self.bench_parameters.faults
              total_nodes = shard_number * (shard_size - faults)
              used_hosts_num = math.ceil(total_nodes / self.nodes_per_host)
              Print.heading(f'Using {used_hosts_num} hosts, {self.nodes_per_host} nodes per host')      
              if len(self.hosts) < used_hosts_num:
                Print.warn('There are not enough instances available')
                return
              used_hosts = self.hosts[:used_hosts_num]
              used_hosts_ips = [host.split(':')[0] for host in used_hosts]
              # print("used_hosts", used_hosts)
              print("used_hosts_ips", used_hosts_ips)
              base_port_for_used_hosts = { host_ip:5000 for host_ip in used_hosts_ips}
              # Upload all configuration files.
              Print.heading(f'\n-Configurating {shard_number} shards, shard size: {shard_size}, faults per shard: {faults}')
              try:
                  committeeList = self._genconfig_and_upload(
                      used_hosts_ips, base_port_for_used_hosts, shard_number, shard_size, self.node_parameters, self.bench_parameters
                  )
              except (subprocess.SubprocessError, GroupException) as e:
                  e = FabricError(e) if isinstance(e, GroupException) else e
                  raise BenchError('Failed to configure nodes', e)   

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
                    
                    # Run the benchmark.
                    for e in range(self.bench_parameters.runs[0]): # repeat `runs` times
                        Print.heading(f'Run {e+1}/{self.bench_parameters.runs}')
                    # for testing multiple epochs
                    # for e in self.bench_parameters.runs:
                        # Print.heading(f'----Run epoch {e}. {self.bench_parameters.runs}')
                        try:
                            self._run_single(
                                used_hosts, 
                                executor_type,
                                e,
                                shard_number, shard_size, total_rate,
                                self.bench_parameters.total_txs, committeeList, 
                                debug
                            )

                            Print.info('Parsing logs...')
                            logger = self._logs(committeeList, e, shard_number, self.bench_parameters.faults, self.bench_parameters.duration, self.bench_parameters.sample_interval)
                            logger.print(PathMaker.result_file(
                              "lan",
                              self.bench_parameters.acc_shard_type,
                              executor_type,
                              self.bench_parameters.state_store_type,
                              shard_number,
                              shard_size,
                              self.bench_parameters.faults,
                              self.bench_parameters.cs_faults,
                              r, 
                              self.bench_parameters.duration,
                              self.bench_parameters.total_txs,
                              self.node_parameters.batch_size,
                            ))
                        except (subprocess.SubprocessError, GroupException, ParseError) as e:
                            self.kill(hosts=used_hosts)
                            if isinstance(e, GroupException):
                                e = FabricError(e)
                            Print.error(BenchError('Benchmark failed', e))
                            continue