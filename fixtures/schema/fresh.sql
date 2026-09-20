-- Transactions 工作空间「当前 schema」基线
--
-- 来源：`sqlite3 transactions.db .schema` 输出的原始 DDL，逐字节复制，未做任何改写。
-- 用途：为**全新工作空间**建库时执行本文件，使库结构与基线逐字节一致
--       （由 `cargo xtask schema-diff` 校验）。
--
-- 纪律（见 AGENTS.md）：本文件只在「数据库文件不存在」时执行一次；
-- 打开既有工作空间时**绝不**执行任何 DDL/DML，只做只读校验。
-- 本仓库不包含任何 schema 迁移逻辑：更早格式的工作空间会被明确拒绝，而不是被就地升级。

CREATE TABLE `tbl_billadm_ledger` (`id` text,`name` text NOT NULL,`description` text,`created_at` integer NOT NULL,`updated_at` integer NOT NULL,PRIMARY KEY (`id`));
CREATE TABLE `tbl_billadm_transaction_record` (`transaction_id` text,`ledger_id` text NOT NULL,`price` integer NOT NULL,`transaction_type` text NOT NULL,`category` text NOT NULL,`description` text,`flags` text,`key_event_date` varchar(10) DEFAULT "",`transaction_at` integer NOT NULL,`created_at` integer NOT NULL,`updated_at` integer NOT NULL,PRIMARY KEY (`transaction_id`));
CREATE INDEX `idx_tr_key_event_date` ON `tbl_billadm_transaction_record`(`key_event_date`);
CREATE INDEX `idx_tr_ledger_at` ON `tbl_billadm_transaction_record`(`ledger_id`,`transaction_at`);
CREATE TABLE `tbl_billadm_transaction_record_tag` (`ledger_id` text NOT NULL,`transaction_id` text NOT NULL,`tag` text NOT NULL);
CREATE INDEX `idx_tr_tag_transaction` ON `tbl_billadm_transaction_record_tag`(`transaction_id`);
CREATE INDEX `idx_tr_tag_ledger` ON `tbl_billadm_transaction_record_tag`(`ledger_id`);
CREATE TABLE `tbl_billadm_category` (`ledger_id` text NOT NULL DEFAULT "",`name` text NOT NULL,`transaction_type` text NOT NULL,`sort_order` integer DEFAULT 0);
CREATE UNIQUE INDEX `idx_category_ledger_name_type` ON `tbl_billadm_category`(`ledger_id`,`name`,`transaction_type`);
CREATE TABLE `tbl_billadm_tag` (`ledger_id` text NOT NULL DEFAULT "",`name` text NOT NULL,`category_transaction_type` text NOT NULL,`sort_order` integer DEFAULT 0);
CREATE UNIQUE INDEX `idx_tag_ledger_name_cattype` ON `tbl_billadm_tag`(`ledger_id`,`name`,`category_transaction_type`);
CREATE TABLE `tbl_billadm_transaction_tpl` (`template_id` text,`ledger_id` text NOT NULL,`template_name` text NOT NULL,`transaction_type` text NOT NULL,`category` text NOT NULL,`tags` text,`flags` text,`description` text,`sort_order` integer DEFAULT 0,`created_at` integer NOT NULL,`updated_at` integer NOT NULL,PRIMARY KEY (`template_id`));
CREATE TABLE `tbl_billadm_chart` (`chart_id` text,`ledger_id` text NOT NULL,`title` text NOT NULL,`granularity` text NOT NULL,`chart_lines` text NOT NULL,`chart_type` text NOT NULL DEFAULT "line",`is_preset` numeric NOT NULL DEFAULT false,`sort_order` integer NOT NULL DEFAULT 0,`created_at` integer NOT NULL,`updated_at` integer NOT NULL,PRIMARY KEY (`chart_id`));
CREATE INDEX `idx_chart_ledger` ON `tbl_billadm_chart`(`ledger_id`);
CREATE TABLE `tbl_billadm_key_event` (`id` text,`date` text NOT NULL,`title` varchar(200),`content` text,`color` varchar(20),`created_at` integer NOT NULL,`updated_at` integer NOT NULL,`ledger_id` varchar(36) DEFAULT "",PRIMARY KEY (`id`));
CREATE UNIQUE INDEX `idx_key_event_ledger_date` ON `tbl_billadm_key_event`(`ledger_id`,`date`);
CREATE TABLE `tbl_billadm_key_event_image` (`id` text,`ledger_id` varchar(36) DEFAULT "",`event_date` text NOT NULL,`file_path` varchar(500) NOT NULL DEFAULT "",`thumb_path` varchar(500) NOT NULL DEFAULT "",`sort_order` integer NOT NULL DEFAULT 0,`created_at` integer NOT NULL,PRIMARY KEY (`id`));
CREATE INDEX `idx_key_event_image_ledger_date` ON `tbl_billadm_key_event_image`(`ledger_id`,`event_date`);
CREATE TABLE `tbl_billadm_diary_entry` (`id` text,`date` text NOT NULL,`content` text,`word_count` integer NOT NULL DEFAULT 0,`mood` varchar(20) DEFAULT "",`created_at` integer NOT NULL,`updated_at` integer NOT NULL,`ledger_id` varchar(36) DEFAULT "",PRIMARY KEY (`id`));
CREATE UNIQUE INDEX `idx_tbl_billadm_diary_entry_ledger_date` ON `tbl_billadm_diary_entry`(`ledger_id`,`date`);
CREATE TABLE `tbl_billadm_stock_account` (`id` text,`ledger_id` varchar(36) DEFAULT "",`principal` integer NOT NULL DEFAULT 0,`created_at` integer NOT NULL,`updated_at` integer NOT NULL,PRIMARY KEY (`id`));
CREATE UNIQUE INDEX `idx_tbl_billadm_stock_account_ledger_id` ON `tbl_billadm_stock_account`(`ledger_id`);
CREATE TABLE `tbl_billadm_stock_fee_setting` (`id` text,`ledger_id` varchar(36) DEFAULT "",`commission_rate` real NOT NULL DEFAULT 0.0002354,`min_commission` integer NOT NULL DEFAULT 500,`stamp_duty_rate` real NOT NULL DEFAULT 0.0005,`transfer_fee_rate` real NOT NULL DEFAULT 0.00001,`created_at` integer NOT NULL,`updated_at` integer NOT NULL,PRIMARY KEY (`id`));
CREATE UNIQUE INDEX `idx_tbl_billadm_stock_fee_setting_ledger_id` ON `tbl_billadm_stock_fee_setting`(`ledger_id`);
CREATE TABLE `tbl_billadm_stock_fund_record` (`id` text,`ledger_id` varchar(36) DEFAULT "",`record_date` varchar(10) NOT NULL,`event_type` varchar(32) NOT NULL DEFAULT "",`event_text` varchar(200) NOT NULL DEFAULT "",`amount_change` integer NOT NULL DEFAULT 0,`cash_balance` integer NOT NULL DEFAULT 0,`net_pnl` integer,`remark` varchar(500) NOT NULL DEFAULT "",`created_at` integer NOT NULL,PRIMARY KEY (`id`));
CREATE INDEX `idx_stock_fund_ledger_date` ON `tbl_billadm_stock_fund_record`(`ledger_id`,`record_date`);
CREATE TABLE `tbl_billadm_stock_position` (`id` text,`ledger_id` varchar(36) DEFAULT "",`stock_code` varchar(16) NOT NULL,`stock_name` varchar(64) NOT NULL DEFAULT "",`quantity` integer NOT NULL DEFAULT 0,`total_cost` integer NOT NULL DEFAULT 0,`realized_pnl` integer NOT NULL DEFAULT 0,`review` varchar(2000) NOT NULL DEFAULT "",`created_at` integer NOT NULL,`updated_at` integer NOT NULL,PRIMARY KEY (`id`));
CREATE UNIQUE INDEX `idx_stock_position_ledger_code` ON `tbl_billadm_stock_position`(`ledger_id`,`stock_code`);
CREATE TABLE `tbl_billadm_stock_trade` (`id` text,`ledger_id` varchar(36) DEFAULT "",`stock_code` varchar(16) NOT NULL,`stock_name` varchar(64) NOT NULL DEFAULT "",`trade_type` varchar(16) NOT NULL DEFAULT "",`round_id` varchar(36) DEFAULT "",`order_id` varchar(36) DEFAULT "",`order_seq` integer NOT NULL DEFAULT 1,`price` integer NOT NULL DEFAULT 0,`lots` integer NOT NULL DEFAULT 0,`shares` integer NOT NULL DEFAULT 0,`amount` integer NOT NULL DEFAULT 0,`fee` integer NOT NULL DEFAULT 0,`commission` integer NOT NULL DEFAULT 0,`stamp_duty` integer NOT NULL DEFAULT 0,`transfer_fee` integer NOT NULL DEFAULT 0,`realized_pnl` integer,`trade_time` integer NOT NULL DEFAULT 0,`remark` varchar(500) NOT NULL DEFAULT "",`created_at` integer NOT NULL,PRIMARY KEY (`id`));
CREATE INDEX `idx_stock_trade_ledger_order` ON `tbl_billadm_stock_trade`(`order_id`);
CREATE INDEX `idx_stock_trade_round` ON `tbl_billadm_stock_trade`(`round_id`);
CREATE INDEX `idx_stock_trade_ledger_code` ON `tbl_billadm_stock_trade`(`ledger_id`,`stock_code`);
CREATE TABLE `tbl_billadm_stock_trade_history` (`id` text,`ledger_id` varchar(36) DEFAULT "",`stock_code` varchar(16) NOT NULL,`stock_name` varchar(64) NOT NULL DEFAULT "",`created_at` integer NOT NULL,`updated_at` integer NOT NULL,PRIMARY KEY (`id`));
CREATE UNIQUE INDEX `idx_stock_trade_history_ledger_code` ON `tbl_billadm_stock_trade_history`(`ledger_id`,`stock_code`);
CREATE TABLE `tbl_billadm_stock_trade_round` (`id` text,`ledger_id` varchar(36) DEFAULT "",`stock_code` varchar(16) NOT NULL,`history_id` varchar(36) NOT NULL,`round_no` integer NOT NULL,`opened_at` integer NOT NULL DEFAULT 0,`closed_at` integer NOT NULL DEFAULT 0,`tag` varchar(16) NOT NULL DEFAULT "分析",`review` varchar(2000) NOT NULL DEFAULT "",`created_at` integer NOT NULL,PRIMARY KEY (`id`));
CREATE UNIQUE INDEX `idx_stock_trade_round_history_no` ON `tbl_billadm_stock_trade_round`(`history_id`,`round_no`);
CREATE INDEX `idx_stock_trade_round_ledger_code` ON `tbl_billadm_stock_trade_round`(`ledger_id`,`stock_code`);
CREATE TABLE `tbl_billadm_stock_trade_tag_setting` (`id` text,`ledger_id` varchar(36) DEFAULT "",`tags` text NOT NULL DEFAULT "[]",`created_at` integer NOT NULL,`updated_at` integer NOT NULL,PRIMARY KEY (`id`));
CREATE UNIQUE INDEX `idx_tbl_billadm_stock_trade_tag_setting_ledger_id` ON `tbl_billadm_stock_trade_tag_setting`(`ledger_id`);
CREATE TABLE `tbl_billadm_schema_migration` (`id` text,`applied_at` integer,PRIMARY KEY (`id`));

-- 以下 4 条记录是迁移登记（这些迁移对**空库**都是空操作）。
-- 本仓库只复刻建库后的最终状态；迁移的执行逻辑在 `tr-store` 的 `migrations` 模块里
-- （打开既有工作空间时按登记表逐个应用），新建库与升级后的库结构一致。
INSERT INTO tbl_billadm_schema_migration (id, applied_at) VALUES ('20260101_key_event_ledger_date_composite_unique', CAST(strftime('%s','now') AS INTEGER));
INSERT INTO tbl_billadm_schema_migration (id, applied_at) VALUES ('20260101_key_event_image_backfill_ledger_id', CAST(strftime('%s','now') AS INTEGER));
INSERT INTO tbl_billadm_schema_migration (id, applied_at) VALUES ('20260918_stock_trade_backfill_order_id', CAST(strftime('%s','now') AS INTEGER));
INSERT INTO tbl_billadm_schema_migration (id, applied_at) VALUES ('20260920_diary_ledger_scope', CAST(strftime('%s','now') AS INTEGER));
