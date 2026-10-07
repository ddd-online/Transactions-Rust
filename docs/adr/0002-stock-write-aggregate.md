# 股票写入聚合成一个 module（`stock::write`）

股票域有一半数据是**派生**的：持仓数量与成本、每笔成交的已实现盈亏、轮次（round）挂接、
资金记录的现金链余额，都由成交流水推出来。但"写入之后谁负责把它们对齐"这件事没有 module 拥有，
四条写入路径各自为政：

* `create_trade_order`（下单 / 建仓）在事务里**增量维护** —— 委托级一次计收费用后按成交额分摊，
  就地改持仓、写资金记录、必要时归档轮次，全程**不重放**；
* `update_trade_fill`（改一笔成交）与 `delete_trade_order`（删一个委托）在事务里改完成交后调
  `rebuild_trades` 整体重放；
* `rollback_latest`（回滚最近一次操作）按操作 `kind` 分派：委托类复用"删一个委托"的事务层，
  资金类删掉那条记录、必要时把本金改回去，再复算现金链。

结果是：加一条新的写入路径，就得自己推一遍"这次要不要重放、重放哪一半"；改重放规则要全文件
找调用点；四条路径的"事务里到底做了什么"要读四个地方（目标文件约 7400 行）。

**决定**：`crates/tr-service/src/stock.rs` 拆出子 module `stock::write`
（`crates/tr-service/src/stock/write.rs`），把"写入 + 对齐派生数据"这条流水线整体搬进去：

* 对外只暴露**意图**：`create_trade_order` / `create_trade` / `update_trade_fill` /
  `delete_trade_order` / `rollback_latest`，外加两条**预演**（`preview_trade_change` /
  `preview_rollback`）；
* **一次意图 = 一个事务**，事务边界在这一层；
* 逐笔事务层（`update_trade_fill_tx` / `delete_trade_order_tx`）、整体重放（`rebuild_trades`）、
  现金链复算（`recalculate_cash_chain`）、轮次归档（`close_round`）与只被它们用到的辅助函数
  都是 module 的内部实现；
* 对齐策略**按操作分流**这件事写在 module 顶部的一张表里 —— 增量维护与整体重放都是保留的
  既有行为，不是待统一的缺陷。

四个入口的名称 / 签名 / 返回与从前逐字相同，`stock.rs` 用 `pub use` 把它们转出去，
因此 `tr-ipc` 与 `xtask` 一行未改，既有断言的期望值一处未改。

## 切在哪条线上

**判据是"写入 + 对齐派生数据"这一条流水线，不是"所有会写库的函数"。**

本金类写入（`set_principal` / `add_principal*` / `add_withdraw*` / `add_interest*`）留在 `stock.rs`：
它们不改成交流水、不触发重放，只是往现金链尾巴上追加一条记录（`#48` 之后插入完要在**同一事务**里
复算整条链，见文末补充）。搬进去会把 module 撑成
"股票域的一切写入"，而它想回答的问题只有一个："改了一笔成交之后，系统做了哪些事"。

**预演跟着搬**，尽管它们不改库：`preview_trade_change` / `preview_rollback` 在事务里**真的执行**
改动、比对前后差异，最后用哨兵错误强制回滚。只有和事务层住在一起，它们才不必把
`*_tx` / `rebuild_trades` 提成兄弟可见 —— 那正是这次要消掉的东西。同理，
`repair_legacy_trade_fund_dates`（旧数据订正）也搬：它的实现就是"条件可证明时整体重放"。

**只被搬走的东西用到的辅助函数跟着搬。** `order_key_of` / `round_meta_key` / `cost_basis_of` /
`day_after` / `normalize_trade_fills` / `trade_order_remark` / `close_round` 全部只有一个使用面
（写入路径），留下就是 `dead_code` —— clippy `-D warnings` 会替我们证明这一点。
共享的（`is_buy` / `unix_to_date` / `get_or_create_*_in` / `log_operation`）留在原处，
子 module 用 `use super::{...}` 取（子 module 能看见父 module 的私有项，不必放宽可见性）。

## 考虑过的其它选项

- **顶层兄弟 module `stock_write.rs`**：判据一样，但只有**子** module 能直接看见 `stock.rs`
  的私有项（`is_buy` / `unix_to_date` / `TradeFill` / `ERR_PREVIEW_ROLLBACK`）；
  兄弟 module 得先把它们提成 `pub(crate)`，等于把写入路径的内部零件漏给整个 crate。
- **分四次迁（对应票据 #2～#5）**：中间态要么让 `rebuild_trades` 留在父 module 给兄弟用、
  要么让预演拿不到事务层，只能临时放宽可见性 —— 与"聚合 module 之外没有调用点"这条验收直接冲突，
  所以一次搬完（`b528939`）。
- **顺手统一"增量维护"与"整体重放"**：两条口径不同（下单是逐笔结算与就地累计，编辑 / 删除是
  整表重算），合并等于改行为，明确不做。

## 后果

- `stock.rs` 只留读取路径（持仓 / 交易列表 / 轮次历史 / 统计取数）、本金类写入与操作记录列表；
  `write.rs` 拥有四条意图、两条预演与全部事务 / 重放机制。
- 新增一条股票写入路径时：加意图入口 + 在 module 顶部那张表里补一行"这次怎么对齐"，
  不需要在别处找重放。
- 边界由编译器兜底：`stock::write` 之外一旦有人直接调 `rebuild_trades` /
  `recalculate_cash_chain` / `*_tx`，要么编不过（私有 / 只对 `stock` 可见），要么得先放宽可见性 ——
  到那时这份 ADR 是该读的第一份材料。
- 测试面没有变化：服务层测试仍走 `stock::` 的公开入口（`pub use` 转出），端到端仍是
  `fixtures/ui-stock.ps1`。落地时的验证：`cargo test -p tr-service` 157 绿；
  `fixtures/test.ps1 -Unit stock,service` 5/5 绿（`ui-stock` 全生命周期 203.6s，含真实行情）；
  `fixtures/test.ps1 -All -SkipBuild` 32/32 绿。

## 补充：资金流水为什么不另立 `FundLedger`（2026-10-07）

候选 3（`#36`）的验收原本写的是"5 个 `query_latest_fund_record` 调用点归零，改走 `FundLedger::latest`"。
做下来发现剩下 3 处服务的是**不同的问题**：`cash_before_records` 要的是"本金那一刻之前的余额"，
现金链复算要的是"按 (日期, 创建时间, ID) 定位到某一条"，最后一笔余额查询要的是"末条"。把它们塞进一个
`FundLedger::latest` 只会让三种语义共用一个名字。

**结论**：资金流水的写入与重算沿用本 ADR 的聚合（`stock::write`），**不另立 owner**；
读的那三种定位各留各的函数，口径判据（链序与余额、支取上限）统一在 `tr_domain::fund`。

## 补充：本金类写入也复算现金链（`#48`，2026-10-07）

`#48` 把现金链的口径收到 `tr_domain::fund::cash_chain` 一处（按 (日期 → 创建时间 → ID) 升序逐条累加，
起点 = 本金 − Σ追加本金），并要求**每次资金记录发生写都在同一事务里复算**：

* `recalculate_cash_chain` 从 `write.rs` 私有改成 `pub(super)` —— `stock.rs` 的三个资金事件
  （追加本金 / 支取 / 利息归本）插入记录后要调它，下单那条路径同样改成"插入占位 → 复算"
  （从前各自算 `prev + amount`，倒填日期时那条记录排不到链末、金额进不了链）。
  即上面「后果」里那句边界现在**只由 `pub(super)` 兜底**，不再是"私有 ⇒ 编译不过"。
* **切分线本身不变**：本金类写入仍留在 `stock.rs`（它们不重放、也不产生成交），
  只是它们与 `write.rs` 现在共用同一个"余额是派生值"的判据。
