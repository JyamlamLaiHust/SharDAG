# Copyright(C) Facebook, Inc. and its affiliates.
from os.path import join


class BenchError(Exception):
    def __init__(self, message, error):
        assert isinstance(error, Exception)
        self.message = message
        self.cause = error
        super().__init__(message)



State_store_type = ["TStore", "MStore"]
Executor_type = ["SharDAG", "Monoxide", "Broker"]
Acc2shard_type = ["Hash", "Graph"]

class PathMaker:

    @staticmethod
    def alias_binary_path():
        return 'node'

    @staticmethod
    def binary_path():
        return join('..', 'target', 'release')

    @staticmethod
    def node_crate_path():
        return join('..', 'node')

    @staticmethod
    def configs_path():
        return 'configs'

    @staticmethod
    def parameters_file():
        return join(PathMaker.configs_path(), '.parameters.json')

    @staticmethod
    def committee_file(shardid):
        return join(PathMaker.configs_path(), f'.committee-{shardid}.json')

    @staticmethod
    def committees_file():
        return join(PathMaker.configs_path(), '.committees.json')

    @staticmethod
    def key_file(i, shardid):
        assert isinstance(i, int) and i >= 0
        assert isinstance(shardid, int) and shardid >= 0
        return join(PathMaker.configs_path(), f'.node-{i}-{shardid}.json')

    @staticmethod
    def dbs_path():
        return 'dbs'

    @staticmethod
    def db_path(nodeid, shardid, j=None):
        assert isinstance(nodeid, int) and nodeid >= 0
        assert (isinstance(j, int) and nodeid >= 0) or j is None # workerid
        assert isinstance(shardid, int) and shardid >= 0
        worker_id = f'-{j}' if j is not None else ''
        return join(PathMaker.dbs_path(), f'.db-{worker_id}-{nodeid}-{shardid}')

    @staticmethod
    def ft_db_path(nodeid, shardid):
        assert isinstance(nodeid, int) and nodeid >= 0
        assert isinstance(shardid, int) and shardid >= 0
        return join(PathMaker.dbs_path(), f'.db-{nodeid}-{shardid}-ft')


    @staticmethod
    def logs_path():
        return 'logs'

    @staticmethod
    def primary_log_file(i, shardid): # nodeid, shardid
        assert isinstance(i, int) and i >= 0
        assert isinstance(shardid, int) and shardid >= 0
        return join(PathMaker.logs_path(), f'primary-{i}-{shardid}.log')

    @staticmethod
    def worker_log_file(i, j, shardid): # nodeid, workerid, shardid
        assert isinstance(i, int) and i >= 0
        assert isinstance(j, int) and j >= 0
        assert isinstance(shardid, int) and shardid >= 0
        return join(PathMaker.logs_path(), f'worker-{j}-{i}-{shardid}.log')

    @staticmethod
    def client_log_file():
        return join(PathMaker.logs_path(), f'client.log')


    # workload 
    @staticmethod
    def input_path():
        # return 'input'
        # return '/home/jaylen/SharDAG/test-workload'
        return '/root/SharDAG/test-workload'

    @staticmethod
    def acc_input_path():
        # return 'input'
        # return '/home/jaylen/SharDAG/test-workload'
        return '/root/SharDAG/test-workload'

    @staticmethod
    def workload_input_file(e): # epoch e contains a fixed number of txs
        assert isinstance(e, int) and e >= 0
        return join(PathMaker.input_path(), f'input-e{e}.csv')

    @staticmethod
    def acc2shard_file_default():
        return join(PathMaker.input_path(), f'default-acc2shard.csv')

    @staticmethod
    def acc2shard_file(e, shard_num): # the initial acc2shard map of epoch e
        assert isinstance(e, int) and e >= 0
        assert isinstance(shard_num, int) and shard_num >= 0
        return join(PathMaker.input_path(), f'acc2shard-e{e}-s{shard_num}.csv')

    @staticmethod
    def actacc2shard_file(e, shard_num): # the initial act-acc2shard map of epoch e
        assert isinstance(e, int) and e >= 0
        assert isinstance(shard_num, int) and shard_num >= 0
        return join(PathMaker.acc_input_path(), f'act-acc2shard-e{e}-s{shard_num}.csv')

    @staticmethod
    def brokers_file():
        return join(PathMaker.input_path(), f'brokers.csv')

    @staticmethod
    def results_path():
        return 'results'

    @staticmethod
    def result_file(bench_type, acc2shard_type, executor_type, state_store_type, shards, nodes, faults, cs_faults, rate, duration, total_txs, batch_size):
        return join(
            PathMaker.results_path(),
            f'{bench_type}-{Acc2shard_type[acc2shard_type]}-{Executor_type[executor_type]}-{State_store_type[state_store_type]}-{shards}-{nodes}-{faults}-{cs_faults}-{rate}-{duration}-{total_txs}-{batch_size}B.txt'
        )


class Color:
    HEADER = '\033[95m'
    OK_BLUE = '\033[94m'
    OK_GREEN = '\033[92m'
    WARNING = '\033[93m'
    FAIL = '\033[91m'
    END = '\033[0m'
    BOLD = '\033[1m'
    UNDERLINE = '\033[4m'


class Print:
    @staticmethod
    def heading(message):
        assert isinstance(message, str)
        print(f'{Color.OK_GREEN}{message}{Color.END}')

    @staticmethod
    def info(message):
        assert isinstance(message, str)
        print(message)

    @staticmethod
    def warn(message):
        assert isinstance(message, str)
        print(f'{Color.BOLD}{Color.WARNING}WARN{Color.END}: {message}')

    @staticmethod
    def error(e):
        assert isinstance(e, BenchError)
        print(f'\n{Color.BOLD}{Color.FAIL}ERROR{Color.END}: {e}\n')
        causes, current_cause = [], e.cause
        while isinstance(current_cause, BenchError):
            causes += [f'  {len(causes)}: {e.cause}\n']
            current_cause = current_cause.cause
        causes += [f'  {len(causes)}: {type(current_cause)}\n']
        causes += [f'  {len(causes)}: {current_cause}\n']
        print(f'Caused by: \n{"".join(causes)}\n')


def progress_bar(iterable, prefix='', suffix='', decimals=1, length=30, fill='█', print_end='\r'):
    total = len(iterable)

    def printProgressBar(iteration):
        formatter = '{0:.'+str(decimals)+'f}'
        percent = formatter.format(100 * (iteration / float(total)))
        filledLength = int(length * iteration // total)
        bar = fill * filledLength + '-' * (length - filledLength)
        print(f'\r{prefix} |{bar}| {percent}% {suffix}', end=print_end)

    printProgressBar(0)
    for i, item in enumerate(iterable):
        yield item
        printProgressBar(i + 1)
    print()
