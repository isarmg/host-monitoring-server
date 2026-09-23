# 07. 当前报告协议、写入与保留

## 7.1 协议层次

配对协议建立设备身份；报告协议提交遥测；管理员 API 查询 Host summary、latest payload 和仍保留的 raw
标量 history；图表模式还可无重复合并 raw 与 hourly aggregate，并返回粒度和来源。公开/管理员/设备三类路由的身份、速率和数据暴露
不同，不能共用一个“万能 token”。

## 7.2 报告不变量

- 当前应用/协议版本精确匹配。
- Host 与 credential 绑定且未撤销。
- report ID 合法且稳定。
- 时间和所有集合/字符串/数值在边界内。
- 相同 Host 重放同一 report ID 返回 `accepted=false` 和首次 `received_at`；跨 Host 复用 ID 返回 409。

当前去重只比较 report ID 的所有 Host，不保存 request fingerprint，也不会验证同一 Host 的重放正文是否
与首次完全相同。因此“相同 ID、同 Host、不同内容必冲突”尚未实现；调用方必须稳定重放同一 spool 文件，
若要把服务端内容一致性提升为保证，需要新增 fingerprint、Schema 与负例，而不能只改文档。

## 7.3 服务端所有权转移

```text
HTTP body -> validation -> bounded queue -> writer batch -> savepoint -> transaction commit -> 202
```

只有进入队列后 writer 才拥有完成责任；客户端断开不取消该责任。队列前失败没有写入承诺，事务失败也
不能返回成功。

## 7.4 Batch 与 Savepoint

writer 将有限报告放入一个事务以降低 fsync 成本；每条使用 savepoint，让一个撤销 credential、冲突 ID
或非法关联不会撤销其他独立报告。Batch 大小和等待时间有上限，避免低流量长期滞留或高流量独占。

## 7.5 Latest 语义

latest 表示每个 Host 当前选定的最新有效报告，不等同于数据库最后插入行。乱序、重试和相同时间需要
稳定裁决。保留任务永远不能删除 latest 所依赖的行，否则在线状态会因清理跳变。

## 7.6 小时聚合

超过 raw 保留期的非 latest 报告按 Host 与 UTC 小时分桶，计算 count/min/max/avg 等受支持标量。聚合
必须幂等记录已纳入范围，再在独立有界事务删除 raw；崩溃在两步之间不能双计或丢计。

## 7.7 保留预算

维护任务按时间、事务、行数和 yield 预算分批工作，启动时执行后按周期运行。不能一次扫描/删除全库，
否则会挤压实时 writer。raw 与 aggregate 期限必须满足当前配置约束。

## 7.8 过载观测

当前可直接取得的证据主要是 `/health/ready` 的 database/retention/writer 布尔值、分类日志、HTTP
429/503，以及 Client `status`/doctor 的 spool 状态。`RetentionMaintenanceStats` 只存在于进程内且启动方
丢弃 handle，Server 也没有 metrics API；队列深度、入队等待、batch/事务延迟、raw/aggregate 行数和
maintenance 用时目前不能由产品端点完整观测。它们是应补的运维 instrumentation，不能写成已交付指标。

## 7.9 Schema 变化

产品只创建不存在的当前数据库，并验证 `product_metadata` 与现场规范 DDL fingerprint。新版本需要新
当前 Schema 时直接更新代码、测试与 identity；只有确实决定支持某个离线输入时，才在外部仓新增明确
adapter/edge。Server 不执行 `ALTER TABLE`，不接受 metadata-free 库，也不会因通用升级引擎存在而自动
获得转换能力。

当前四元身份是 `host-monitoring` / `0.9.26` / schema revision `7` /
`5c4a32f3f1813e6e6ef528b55e25e912bfe0191f79332ad5538943746c8f17a3`。管理员列为
`_sarmg_administrators.username`；DDL CHECK/UNIQUE 与启动时 Foundation canonical username/current Argon2id 加载
检查是两层不同防线。`doctor` 只覆盖 Schema、integrity、foreign key 和 retention 结构，不应被当作账户
内容校验。

## 7.10 协议变更验收

除正例外必须测试：unknown field/版本、极值、空集合、超长文本、大整数、同 Host ID 重放的当前实际
语义、跨 Host ID 冲突、撤销竞态、
队列满、writer 停止、事务失败、乱序与保留崩溃恢复。
