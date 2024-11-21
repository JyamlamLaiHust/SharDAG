import os
from re import findall
from matplotlib import pyplot as plt
import numpy as np
import pandas as pd
import seaborn as sns
import matplotlib.ticker as ticker
from matplotlib.pyplot import MultipleLocator
from os.path import join
import os



class Plot:
  def __init__(self):
      self.input_path = 'results'
      if not os.path.exists(self.input_path):
        raise FileNotFoundError(f"The directory '{self.input_path}' does not exist. Please run the benchmarks first to get enough results")  
      
      self.data = 'data'
      if not os.path.exists(self.data):
        os.makedirs(self.data)
      
      self.color_list = sns.color_palette('bright')
      self.color = [self.color_list[0], self.color_list[2], self.color_list[3]]
      self.hue_order = ['Monoxide-BS', 'BrokerChain-BS', 'SharDAG']      
      self.pattern=['//','\\\\', '//\\\\']


  def _parse_res_single(self, log):
    tmp = findall(r'End-to-end execution TPS: ([^ ]+) tx/s', log) 
    tps = [int(d.replace(',','')) for d in tmp] 
    tmp = findall(r'End-to-end execution latency: ([^ ]+) ms', log) 
    latency = [int(d.replace(',','')) for d in tmp] 
    return tps, latency


  def _parse_res(self, res_path, input_rate):
    print(f'Parsing the result file (input_rate={input_rate})')
    out_f = open(res_path, 'w+')
    out_f.write('type,input_rate,method,nodes_per_host,total_nodes,shard_num,shard_size,cs_faults,tps,latency\n')

    files = os.listdir(self.input_path)
    for i in files:
        file_path = os.path.join(self.input_path, i)
        if i.split('.')[1] == 'txt':
          t = i.split('.')[0].split('-')
          if int(t[8]) == input_rate:
            # print(file_path)
            with open(file_path, 'r') as f:
                res = f.read()
                method = t[2]
                if method == 'Broker':
                  method = 'BrokerChain-BS'
                elif method == 'Monoxide':
                  method = 'Monoxide-BS'
                tps, latency = self._parse_res_single(res)
                for i in range(len(tps)):
                  out_f.write(f'{t[1]},{t[8]},{method},8,{int(t[4])*int(t[5])},{t[4]},{t[5]},{t[7]},{tps[i]},{latency[i]}\n')
    out_f.close()


  def draw_tps(self, df, output_file):
    g = sns.barplot(
      data=df,
      x="shard_num", y="tps", 
      hue="method", hue_order=self.hue_order, 
      linewidth=1,
      edgecolor='black',
      palette=self.color,
      errorbar=None
    )
    plt.grid(True, linestyle='--', linewidth=0.5, color='gray') # 网格线
    g.set_xlabel("Shard number")
    g.set_ylabel("Throughput (TPS)")

    bars = g.patches
    hatches=np.repeat(self.pattern,7)
    for pat,bar in zip(hatches,bars):
      bar.set_hatch(pat)
      # bar.set_linewidth(3)

    g.yaxis.set_major_formatter(ticker.FuncFormatter(lambda y, pos: '{:,.0f}'.format(y/1000) + 'K'))
    g.legend(
      loc='upper left', 
      bbox_to_anchor=(-0.02,1.03), 
      frameon=True, 
      labelspacing=0.15, 
      handletextpad=0.3, 
      borderpad=0.2, 
      prop = {'size':28}
    )
    y_major_locator=MultipleLocator(4000)
    ax=plt.gca()
    ax.yaxis.set_major_locator(y_major_locator)

    # g.legend_.remove()
    plt.savefig(output_file, bbox_inches='tight', dpi=600)
    plt.clf()


  def draw_latency(self, df, output_file):
    g = sns.barplot(
      data=df,
      x="shard_num", y="latency", 
      hue="method", hue_order=self.hue_order, 
      linewidth=1,
      edgecolor='black',
      palette=self.color,
      errorbar=None
    )
    plt.grid(True, linestyle='--', linewidth=0.5, color='gray') # 网格线
    g.set_xlabel("Shard number")
    g.set_ylabel("Latency (sec)")

    bars = g.patches
    hatches=np.repeat(self.pattern,7)
    for pat,bar in zip(hatches,bars):
      bar.set_hatch(pat)
      # bar.set_linewidth(3)
    
    g.yaxis.set_major_formatter(ticker.FuncFormatter(lambda y, pos: '{:,.0f}'.format(y/1000)))
    g.legend(
      loc='upper left', 
      bbox_to_anchor=(-0.02,1.03), 
      frameon=True,
      labelspacing=0.15, 
      handletextpad=0.3, 
      borderpad=0.2, 
      prop = {'size':28}
    )
    y_major_locator=MultipleLocator(20000)
    ax=plt.gca()
    ax.yaxis.set_major_locator(y_major_locator)

    # sns.move_legend(g, "lower center", bbox_to_anchor=(.5, 1), ncol=3, title=None, frameon=False)
    # g.legend_.remove()
    plt.savefig(output_file, bbox_inches='tight', dpi=600)
    plt.clf()
    
    
  def draw_tps_latency(self, input_rates):
    sns.set_theme(
      style="ticks", 
      font_scale=3.0,
      font='Arial',
      rc={
        'figure.figsize':(8,5),
        "axes.spines.right": True, "axes.spines.top": True, "axes.spines.left": True,
      }
    )
    
    for input_rate in input_rates:
      res_path = join(self.data, f'res-hash-{input_rate}-300s-tps-latency.csv')
      self._parse_res(res_path, input_rate)
      
      input_file = res_path.split('/')[1].split('.')[0]
      df = pd.read_csv(res_path)
      
      print(f'tps (input_rate={input_rate})')
      out_file = join(self.data, f'{input_file}-tps.png')
      self.draw_tps(df, out_file)
      print(f'latency (input_rate={input_rate})')
      out_file = join(self.data, f'{input_file}-latency.png')
      self.draw_latency(df, out_file)