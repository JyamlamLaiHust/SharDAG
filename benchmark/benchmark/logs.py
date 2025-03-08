# Copyright(C) Facebook, Inc. and its affiliates.
from datetime import datetime
from glob import glob
from multiprocessing import Pool
from os.path import join
from random import sample
from re import findall, search
from statistics import mean
import time
from .utils import Print


class ParseError(Exception):
    pass


class LogParser:
    def __init__(self, clients, primaries, workers, epoch, faults, cs_faults, total_txs, shard_nums, duration, sample_interval, res_ledger_MB, res_state_MB):
        ## storage cost
        assert isinstance(res_ledger_MB, list)
        self.res_ledger_MB = res_ledger_MB
        assert isinstance(res_state_MB, list)
        self.res_state_MB = res_state_MB

        inputs = [primaries, workers]
        # 判断inputs中的每一个x是否为list类型，所有元素都通过检查则返回true
        assert all(isinstance(x, list) for x in inputs)
        assert all(isinstance(x, str) for y in inputs for x in y)
        assert all(x for x in inputs)
        self.duration = duration
        self.faults = faults
        self.cs_faults = cs_faults
        self.total_txs = total_txs
        self.sample_interval = sample_interval
        self.epoch = epoch

        # 根据 faults 类型初始化分片委员会相关参数
        if isinstance(faults, int):
            self.real_committee_size = len(primaries) / shard_nums # 真实委员会大小
            self.committee_size = int((len(primaries) + int(faults)) / shard_nums) # 理论委员会大小
            self.shard_nums = shard_nums
            self.workers = len(workers) // len(primaries)
        # 假如 faults 不是整数，用占位符表示未知值
        else:
            self.committee_size = '?'
            self.workers = '?'

        # Parse the clients logs.
        try:
            # 多进程池解析客户端日志
            with Pool() as p:
                results = p.map(self._parse_clients, clients)
        except (ValueError, IndexError, AttributeError) as e:
            raise ParseError(f'Failed to parse clients\' logs: {e}')
        # 解包客户端日志解析结果，将这些元组的同一位置的元素组合成新的元组
        size, rate, self.start, misses, self.sent_samples, total_sent_txs \
            = zip(*results)
        self.size = size[0]
        self.misses = sum(misses)
        self.rate = sum(rate)
        self.total_sent_txs = sum(total_sent_txs)   

        # Parse the primaries logs.
        try:
            with Pool() as p:
                results = p.map(self._parse_primaries, primaries)
        except (ValueError, IndexError, AttributeError) as e:
            raise ParseError(f'Failed to parse nodes\' logs: {e}')
        proposals, commits, self.configs, primary_ips = zip(*results)
        self.proposals = self._merge_results([x.items() for x in proposals]) # {batch.digest, timestamp}
        self.commits = self._merge_results([x.items() for x in commits]) # {batch.digest, timestamp}
        
        # Parse the workers logs.
        try:
            with Pool() as p:
                results = p.map(self._parse_workers, workers) # a list
        except (ValueError, IndexError, AttributeError) as e:
            raise ParseError(f'Failed to parse workers\' logs: {e}')
        workers_ips, sizes, self.submitted_tx_nums, self.total_packaged_external_txs, self.received_samples, executed_txs, self.execution_end_t, self.total_general_txs, self.total_external_txs, self.total_cs_txs, self.total_commit_txs, self.total_aborted_txs, self.cache_storage_cost, append_delay, append_type = zip(*results) # batch

        # 过滤并生成已提交批次的大小字典
        # sizes: ({batch.digest: batch size}, {}, {}, {})
        self.sizes = { # x is a dict, k is batch.digest, v is batch size
            k: v for x in sizes for k, v in x.items() if k in self.commits 
        } # committed batch
        # 判断主节点和工作节点是否共置
        # Determine whether the primary and the workers are collocated.
        self.collocate = set(primary_ips) == set(workers_ips)

        # 计算打包的外部交易总数
        self.total_packaged_external_txs = int(sum(self.total_packaged_external_txs))
        # print("total_packaged_external_txs: ", self.total_packaged_external_txs)

        # 合并执行交易的结果数据
        self.executed_txs = self._merge_results([x.items() for x in executed_txs]) # {sample tx, execution timestamp}

        # 限制缓存存储成本列表的长度为分片数
        self.cache_storage_cost = list(self.cache_storage_cost)[:self.shard_nums]
        # 初始化缓存存储成本的统计值
        self.avg_avatar_cost = 0
        self.max_avatar_cost = 0
        avg_avatar_cost = []
        max_avatar_cost = []
        # 遍历计算每个分片的平均和最大交易成本
        for tmp in self.cache_storage_cost:
          if len(tmp) != 0:
            avg_avatar_cost.append(mean(tmp))
            max_avatar_cost.append(max(tmp))
        # 计算总体的平均和最大存储成本
        if len(avg_avatar_cost) != 0:
          self.avg_avatar_cost = mean(avg_avatar_cost)
          self.max_avatar_cost = max(max_avatar_cost)
        # print(self.avg_avatar_cost)
        # print(self.max_avatar_cost)

        # 计算每种类型交易的总数（按委员会大小归一化）
        self.total_general_txs = int(sum(self.total_general_txs) / self.committee_size)
        self.total_external_txs = int(sum(self.total_external_txs) / self.committee_size)
        self.total_cs_txs = int(sum(self.total_cs_txs) / self.committee_size)
        self.total_commit_txs = int(sum(self.total_commit_txs) / self.committee_size)
        self.total_aborted_txs = int(sum(self.total_aborted_txs) / self.committee_size)

        # 计算追加延迟的平均值
        # print("all append delay: ", append_delay)
        avg_delay = []
        for tmp in append_delay:
          if len(tmp) != 0:
             avg_delay.append(mean(tmp))
        self.avg_csmsg_append_delay = 0
        if len(avg_delay) != 0:
          self.avg_csmsg_append_delay = mean(avg_delay)
        # print(avg_delay)
        # print(self.avg_csmsg_append_delay)

        # 设置追加类型
        self.append_type = append_type[0]
        
        # Check whether clients missed their target rate.
        if self.misses != 0:
            Print.warn(
                f'Clients missed their target rate {self.misses:,} time(s)'
            )

    def _merge_results(self, input):
        # Keep the earliest timestamp.
        merged = {}
        for x in input:
            for k, v in x: # k是批次标识，v是时间戳
                if not k in merged or merged[k] > v:
                    merged[k] = v
        return merged

    def _parse_clients(self, log):
        # 检查是否有 Error 关键字，有则说明客户端异常崩溃
        if search(r'Error', log) is not None:
            raise ParseError('Client(s) panicked')

        # 提取交易大小和速率
        size = int(search(r'Transactions size: (\d+)', log).group(1)) 
        rate = int(search(r'Transactions rate: (\d+)', log).group(1))

        # 提取日志中的启动时间，并转换为 POSIX 格式
        tmp = search(r'\[(.*Z) .* Start ', log).group(1)
        start = self._to_posix(tmp)

        # 统计未达到目标速率的次数
        misses = len(findall(r'rate too high', log))

        # 提取日志中所有采样交易的信息
        tmp = findall(r'\[(.*Z) .* sample transaction (\d+)', log)
        samples = {int(s): self._to_posix(t) for t, s in tmp} # all sample tx -> dict. samples = {sample_tx.counter: timestamp}
        total_sent_sample_txs = len(tmp)

        # 提取采样间隔
        sample_interval = int(search(r'sample interval: one per (\d+) txs', log).group(1))
        total_sent_txs = total_sent_sample_txs * sample_interval

        return size, rate, start, misses, samples, total_sent_txs

    def _parse_primaries(self, log):
        # 检查日志中是否有panicked或者error，如有则说明节点崩溃
        if search(r'(?:panicked|Error)', log) is not None:
            raise ParseError('Primary(s) panicked')
        # 查找批次创建时间
        tmp = findall(r'\[(.*Z) .* Created B\d+\([^ ]+\) -> ([^ ]+=)', log)
        tmp = [(d, self._to_posix(t)) for t, d in tmp] # [(batch.digest, timestamp)]
        # 合并所有的批次创建时间，返回最早的时间
        proposals = self._merge_results([tmp])

        # 查找批次提交时间
        tmp = findall(r'\[(.*Z) .* Committed B\d+\([^ ]+\) -> ([^ ]+=)', log)
        tmp = [(d, self._to_posix(t)) for t, d in tmp] # [(batch.digest, timestamp)]
        commits = self._merge_results([tmp]) # The earliest time the transaction was submitted

        # 提取配置项，使用正则表达式从日志中提取各个配置的数值
        configs = {
            'header_size': int(
                search(r'Header size .* (\d+)', log).group(1)
            ),
            'max_header_delay': int(
                search(r'Max header delay .* (\d+)', log).group(1)
            ),
            'gc_depth': int(
                search(r'Garbage collection depth .* (\d+)', log).group(1)
            ),
            'sync_retry_delay': int(
                search(r'Sync retry delay .* (\d+)', log).group(1)
            ),
            'sync_retry_nodes': int(
                search(r'Sync retry nodes .* (\d+)', log).group(1)
            ),
            'batch_size': int(
                search(r'Batch size .* (\d+)', log).group(1)
            ),
            'max_batch_delay': int(
                search(r'Max batch delay .* (\d+)', log).group(1)
            ),
        }

        # 提取节点创建的 ip 地址
        ip = search(r'booted on (\d+.\d+.\d+.\d+)', log).group(1)

        return proposals, commits, configs, ip

    def _parse_workers(self, log):
        # 检查日志是否损坏或报错
        if search(r'(?:panic|Error)', log) is not None:
            raise ParseError('Worker(s) panicked')

        # 提取工作节点的ip地址和附加类型
        ip = search(r'booted on (\d+.\d+.\d+.\d+)', log).group(1)
        append_type = search(r'append_type: ([^ ]+) ', log).group(1)

        ### batch maker
        tmp = findall(r'Batch ([^ ]+) contains (\d+) B and (\d+) external txs, (\d+) total_packaged_external_txs', log)
        sizes = {d: int(s) for d, s, nums, total_nums in tmp} # {batch.digest: batch size}
        submitted_tx_nums = {d: int(nums) for d, s, nums, total_nums in tmp} # {batch.digest: external txs}
        total_packaged_external_txs = 0
        if len(tmp) != 0:
          _, _, _, total_packaged_external_txs = tmp[-1]
        total_packaged_external_txs = int(total_packaged_external_txs)
        
        tmp = findall(r'Batch ([^ ]+) contains sample tx (\d+)', log) 
        samples = {int(s): d for d, s in tmp} # {sample_tx.counter: batch.digest}

        ### executor
        # get tx_executed_time
        tmp = findall(r'\[(.*Z) .* Successfully execute sample tx (\d+) in batch', log) # t, tx        
        tmp = [(int(tx), self._to_posix(t)) for t, tx in tmp] # [(sample tx, executed_timestamp)]
        executed_txs = self._merge_results([tmp]) # executed_txs is a list. {sample tx: executed_timestamp}

        tmp = findall(r'\[(.*Z) .* total_general_txs: (\d+), total_external_txs: (\d+), total_cross_shard_txs: (\d+), total_commit_txs: (\d+), total_aborted_txs: (\d+)', log)
        end_t, total_general_txs, total_external_txs, total_cs_txs, total_commit_txs, total_aborted_txs = tmp[-1]
        end_t = self._to_posix(end_t)
        total_general_txs = int(total_general_txs)
        total_external_txs = int(total_external_txs)
        total_cs_txs = int(total_cs_txs)
        total_commit_txs = int(total_commit_txs)
        total_aborted_txs = int(total_aborted_txs)

        tmp = findall(r'Storage cost of acc caching: (\d+) KB', log)
        cache_storage_cost = [int(cost) for cost in tmp]
        # print(cache_storage_cost)
        # print("max_storage_cost: ", max(cache_storage_cost))

        tmp = findall(r'append delay: (\d+) ms', log) # t, tx       
        append_delay = [int(x) for x in tmp]
        # print("csmsg appending delay", append_delay)

        return ip, sizes, submitted_tx_nums, total_packaged_external_txs, samples, executed_txs, end_t, total_general_txs, total_external_txs, total_cs_txs, total_commit_txs, total_aborted_txs, cache_storage_cost, append_delay, append_type

    def _to_posix(self, string):
        x = datetime.fromisoformat(string.replace('Z', '+00:00'))
        return datetime.timestamp(x)

    # 计算bps（字节每秒）、tps（交易每秒）和duration（持续时间）
    def _consensus_throughput(self):
        if not self.commits:
            return 0, 0, 0
        start, end = min(self.proposals.values()), max(self.commits.values()) # start when the first batch was packaged in a header 
        duration = end - start
        # print("consensus duration: ", duration)
        bytes = sum(self.sizes.values()) # self.sizes: committed batch size
        bps = bytes / duration
        total_committed_tx_nums = sum(txs_nums for res in self.submitted_tx_nums for batch, txs_nums in res.items() if batch in self.commits)
        tps = total_committed_tx_nums / duration
        return tps, bps, duration

    # 计算共识延迟
    def _consensus_latency(self): # the commit latency of batch
        latency = [c - self.proposals[d] for d, c in self.commits.items()] # commit_timestamp - proposal_timestamp
        return mean(latency) if latency else 0

    # 计算端到端吞吐量
    def _end_to_end_throughput(self):
        if not self.commits: # no batch was committed
            return 0, 0, 0
        start, end = min(self.start), max(self.commits.values()) 
        duration = end - start
        # print("end to end duration: ", duration)

        bytes = sum(self.sizes.values())
        bps = bytes / duration

        total_committed_tx_nums = sum(txs_nums for res in self.submitted_tx_nums for batch, txs_nums in res.items() if batch in self.commits)
        tps = total_committed_tx_nums / duration
        return tps, bps, duration

    # 计算端到端的延迟
    def _end_to_end_latency(self):
        latency = []
        for sent, received in zip(self.sent_samples, self.received_samples): # client - send, worker - received, primary - commit
            for tx_id, batch_id in received.items():
                if batch_id in self.commits: # sample tx's batch was committed
                    assert tx_id in sent  # We receive txs that we sent.
                    start = sent[tx_id]
                    end = self.commits[batch_id]
                    latency += [end - start] # the latency of a sample tx
        return mean(latency) if latency else 0

    # 计算端到端执行吞吐量：执行的 TPS 和执行持续时间
    def _end_to_end_execution_throughput(self):
        if not self.commits:
            return 0, 0, 0
        start, end = min(self.start), max(self.execution_end_t) 
        exec_duration = end - start
        # print("end to end execution duration: ", exec_duration)
        tps = self.total_commit_txs / exec_duration
        return tps, exec_duration

    # 计算端到端执行延迟
    def _end_to_end_execution_latency(self):
        latency = []
        all_sent_sample_txs = {tx_id: t for res in self.sent_samples for tx_id, t in res.items()}
        # print(all_sent_sample_txs)
        for tx_id, t in self.executed_txs.items():
          # print(tx_id)
          assert tx_id in all_sent_sample_txs
          start = all_sent_sample_txs[tx_id]
          end = t 
          latency += [end - start] # the latency of a sample tx from issuing to being executed
        return mean(latency) if latency else 0, latency

    # 汇总结果并格式化输出
    def result(self):
        header_size = self.configs[0]['header_size']
        max_header_delay = self.configs[0]['max_header_delay']
        gc_depth = self.configs[0]['gc_depth']
        sync_retry_delay = self.configs[0]['sync_retry_delay']
        sync_retry_nodes = self.configs[0]['sync_retry_nodes']
        batch_size = self.configs[0]['batch_size']
        max_batch_delay = self.configs[0]['max_batch_delay']

        consensus_latency = self._consensus_latency() * 1_000
        consensus_tps, consensus_bps, _ = self._consensus_throughput()

        end_to_end_tps, end_to_end_bps, duration = self._end_to_end_throughput()
        end_to_end_latency = self._end_to_end_latency() * 1_000

        end_to_end_execution_tps, exec_duration = self._end_to_end_execution_throughput()
        end_to_end_execution_latency, latencys = self._end_to_end_execution_latency()

        end_to_end_execution_latency = end_to_end_execution_latency * 1_000
        latencys = [ i * 1_000 for i in latencys]

        f_tps = self.total_commit_txs / duration
        # print("false end-to-end execution tps: ", f_tps)

        return (
            '\n'
            '-----------------------------------------\n'
            ' SUMMARY:\n'
            '-----------------------------------------\n'
            ' + CONFIG:\n'
            f' Total node number: {self.shard_nums * self.committee_size} node(s)\n'
            f' Shard number: {self.shard_nums} shard(s)\n'
            f' Shard size: {self.committee_size} node(s)\n'
            f' Faults: {self.faults} node(s)\n'
            f' CS faults: {self.cs_faults} node(s)\n'
            f' Worker(s) per node: {self.workers} worker(s)\n'
            f' Collocate primary and workers: {self.collocate}\n'
            f' Tx size: {self.size:,} B\n'
            f' Epoch: {self.epoch} \n'
            f' Total txs: {self.total_txs:,} \n'
            f' Total tx rate: {self.rate:,} tx/s\n'
            f' Total txs (actual): {int(self.total_sent_txs):,}\n'
            f' Execution time: {round(exec_duration):,} s\n'
            f' Header size: {header_size:,} B\n'
            f' Max header delay: {max_header_delay:,} ms\n'
            f' GC depth: {gc_depth:,} round(s)\n'
            f' Sync retry delay: {sync_retry_delay:,} ms\n'
            f' Sync retry nodes: {sync_retry_nodes:,} node(s)\n'
            f' Batch size: {batch_size:,} B\n'
            f' Max batch delay: {max_batch_delay:,} ms\n'
            '\n'
            ' + RESULTS:\n'
            '------Consensus Performance------\n'
            f' Consensus TPS: {round(consensus_tps):,} tx/s\n'
            f' Consensus BPS: {round(consensus_bps):,} B/s\n'
            f' Consensus latency: {round(consensus_latency):,} ms\n'
            '\n'
            '------End-to-end Consensus Performance------\n'
            f' End-to-end TPS: {round(end_to_end_tps):,} tx/s\n'
            f' End-to-end BPS: {round(end_to_end_bps):,} B/s\n'
            f' End-to-end latency: {round(end_to_end_latency):,} ms\n'
            '\n'
            '------End-to-end Execution Performance------\n'
            f' End-to-end execution TPS: {round(end_to_end_execution_tps):,} tx/s\n'
            f' End-to-end execution latency: {round(end_to_end_execution_latency):,} ms\n'
            '\n'
            f' Total packaged external txs: {int(self.total_packaged_external_txs)} txs\n'            
            f' Total precessed external txs: {int(self.total_external_txs)} txs\n'
            f' Total committed txs: {self.total_commit_txs} txs\n'
            f' Cross-shard txs: {self.total_cs_txs}\n'
            f' Cross-shard tx rate: {(self.total_cs_txs/self.total_external_txs) * 100}%\n'
            f' Total general txs: {self.total_general_txs} txs\n'
            f' Total external txs/Total general txs: {(self.total_external_txs/self.total_general_txs) * 100}%\n'
            '\n'
            '------CSMsg appending delay------\n'
            f' Append type: {self.append_type}\n'
            f' Avg appending delay: {round(self.avg_csmsg_append_delay)} ms\n'
            '\n'
            '------Account cache storage cost------\n'
            f' Avg avatar cost (KB): {round(self.avg_avatar_cost)}\n'
            f' Max avatar cost (KB): {round(self.max_avatar_cost)}\n'
            '\n'
            '------Storage cost------\n'
            f' Ledger per node (MB): {mean(self.res_ledger_MB)}\n'
            f' State per node (MB): {mean(self.res_state_MB)}\n'
            '-----------------------------------------\n'
        )

    # 保存结果到文件
    def print(self, filename):
        assert isinstance(filename, str)
        with open(filename, 'a') as f: 
            res = self.result()
            print(time.strftime('%Y-%m-%d %H:%M:%S',time.localtime(time.time())), "Save results to file!")
            # print("Save results to file!")
            f.write(res)
            print('-----------------------------------------\n')

    # 处理日志数据并返回 LogParser 实例
    @classmethod
    def process(cls, directory, epoch, shard_nums, faults, cs_faults, total_txs, duration, sample_interval, res_ledger_MB, res_state_MB):
        assert isinstance(directory, str)

        clients = [] 
        for filename in sorted(glob(join(directory, 'client.log'))):
            with open(filename, 'r') as f:
                clients += [f.read()]
        primaries = []
        for filename in sorted(glob(join(directory, 'primary-*.log'))):
            with open(filename, 'r') as f:
                primaries += [f.read()]
        workers = [] # all workers in the sharded system
        for filename in sorted(glob(join(directory, 'worker-*.log'))):
            with open(filename, 'r') as f:
                workers += [f.read()]
        

        return cls(clients, primaries, workers, epoch, faults, cs_faults, total_txs, shard_nums, duration, sample_interval, res_ledger_MB, res_state_MB)

if __name__ == "__main__":

  LogParser.process('logs', faults=0, shard_nums=2)
