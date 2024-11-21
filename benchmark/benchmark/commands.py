# Copyright(C) Facebook, Inc. and its affiliates.
from os.path import join
from .utils import PathMaker


class CommandMaker:

    @staticmethod
    def clean_binary():
      return 'rm node'

    @staticmethod
    def cleanup_config():
        return f'rm -r {PathMaker.configs_path()} ; mkdir -p {PathMaker.configs_path()} ; mkdir -p {PathMaker.results_path()}'

    @staticmethod
    def clean_dbs():
        return f'rm -r {PathMaker.dbs_path()} ; mkdir -p {PathMaker.dbs_path()}'

    @staticmethod
    def clean_logs():
        return f'rm -r {PathMaker.logs_path()} ; mkdir -p {PathMaker.logs_path()}'

    @staticmethod
    def clean_input():
        return f'rm -r {PathMaker.logs_path()} ; mkdir -p {PathMaker.logs_path()}'

    @staticmethod
    def clean_inputs(nodes, shard_number, r):
        if nodes == 0:
          return f'rm -r {PathMaker.input_path()} ; mkdir -p {PathMaker.input_path()}'
        else:
          return f'rm -r {join(PathMaker.input_path(), PathMaker.input_path_bench(nodes, shard_number, r))} ; mkdir -p {join(PathMaker.input_path(), PathMaker.input_path_bench(nodes, shard_number, r))}'

    @staticmethod
    def compile():
        return 'cargo build --quiet --release --features benchmark'
      

    @staticmethod
    def generate_key(filename):
        assert isinstance(filename, str)
        return f'./node generate_keys --filename {filename}'

    @staticmethod
    def run_primary(parameters, committees, shardid, keys, store, debug=False):
        assert isinstance(keys, str)
        assert isinstance(committees, str)
        assert isinstance(parameters, str)
        assert isinstance(debug, bool)
        v = '-vvv' if debug else '-vv'
        return (f'./node {v} run --keys {keys} --committee {committees} --shardid {shardid} '
                f'--store {store} --parameters {parameters} primary')

    @staticmethod
    def run_worker(executor_type, state_store_type, acc_shard_type, append_type, parameters, committees, shardid, id, cs_faults, is_cs_fault, keys, store, ftstore, acc2shard, actacc2shard, epoch, debug=False):
        assert isinstance(keys, str)
        assert isinstance(committees, str)
        assert isinstance(parameters, str)
        assert isinstance(acc2shard, str)
        assert isinstance(debug, bool)
        v = '-vvv' if debug else '-vv'
        return (f'./node {v} run --keys {keys} --committee {committees} --shardid {shardid} '
                f'--store {store} --parameters {parameters} worker --id {id} --cs_faults {cs_faults} --is_cs_fault {is_cs_fault} --acc2shard {acc2shard} --actacc2shard {actacc2shard} --ftstore {ftstore} --state_store_type {state_store_type} --executor_type {executor_type} --acc_shard_type {acc_shard_type} --append_type {append_type} --epoch {epoch} ')

    @staticmethod
    def run_client(executor_type, acc_shard_type, committees, client_addr, size, rate, total_txs, workload, acc2shard, brokers, nodes, epoch):
        assert isinstance(size, int) and size > 0
        assert isinstance(rate, int) and rate >= 0
        assert isinstance(nodes, list)
        assert all(isinstance(x, str) for x in nodes)
        assert isinstance(workload, str)
        nodes = f'--nodes {" ".join(nodes)}' if nodes else ''
        return f'./benchmark_client --executor_type {executor_type} --acc_shard_type {acc_shard_type} --committee {committees} --client_addr {client_addr} --size {size} --rate {rate} --totaltxs {total_txs} --workload {workload} --acc2shard {acc2shard} --brokers {brokers} --epoch {epoch} {nodes}'

    @staticmethod
    def kill():
        return 'tmux kill-server'

    @staticmethod
    def alias_binaries(origin):
        assert isinstance(origin, str)
        node, client = join(origin, 'node'), join(origin, 'benchmark_client')
        return f'rm node ; rm benchmark_client ; ln -s {node} . ; ln -s {client} .'