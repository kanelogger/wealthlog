-- M1: investment ownership and source tracking. This stores no new asset facts.
-- Historical identities deliberately do not cascade from accounts/activities.
CREATE TABLE fund_purposes (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL CHECK(length(trim(name))>0),
    category TEXT NOT NULL,
    currency TEXT NOT NULL CHECK(length(currency)=3 AND currency NOT GLOB '*[^A-Z]*'),
    target_amount TEXT CHECK (target_amount IS NULL OR (target_amount <> '' AND target_amount NOT GLOB '*[^0-9.]*'
        AND target_amount GLOB '[0-9]*' AND target_amount NOT LIKE '%.%.%'
        AND (target_amount = '0' OR substr(target_amount,1,1) <> '0' OR substr(target_amount,2,1) = '.')
        AND (instr(target_amount,'.') = 0 OR (substr(target_amount,-1) <> '.' AND substr(target_amount,-1) <> '0')))),
    use_on TEXT,
    liquidity TEXT,
    minimum_cash TEXT CHECK (minimum_cash IS NULL OR (minimum_cash <> '' AND minimum_cash NOT GLOB '*[^0-9.]*'
        AND minimum_cash GLOB '[0-9]*' AND minimum_cash NOT LIKE '%.%.%'
        AND (minimum_cash = '0' OR substr(minimum_cash,1,1) <> '0' OR substr(minimum_cash,2,1) = '.')
        AND (instr(minimum_cash,'.') = 0 OR (substr(minimum_cash,-1) <> '.' AND substr(minimum_cash,-1) <> '0')))),
    loss_budget TEXT CHECK (loss_budget IS NULL OR (loss_budget <> '' AND loss_budget NOT GLOB '*[^0-9.]*'
        AND loss_budget GLOB '[0-9]*' AND loss_budget NOT LIKE '%.%.%'
        AND (loss_budget = '0' OR substr(loss_budget,1,1) <> '0' OR substr(loss_budget,2,1) = '.')
        AND (instr(loss_budget,'.') = 0 OR (substr(loss_budget,-1) <> '.' AND substr(loss_budget,-1) <> '0')))),
    single_asset_limit_bps INTEGER CHECK(single_asset_limit_bps BETWEEN 0 AND 10000),
    is_archived INTEGER NOT NULL DEFAULT 0 CHECK(is_archived IN (0,1)),
    version INTEGER NOT NULL DEFAULT 1 CHECK(version>0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE fund_allocation_operations (
    id TEXT PRIMARY KEY NOT NULL,
    operation_kind TEXT NOT NULL CHECK(operation_kind IN
        ('ALLOCATE','REALLOCATE','TRADE','TRANSFER','RECONCILE','REVERSE')),
    request_id TEXT NOT NULL UNIQUE REFERENCES business_requests(id) DEFERRABLE INITIALLY DEFERRED,
    reason TEXT NOT NULL CHECK(length(trim(reason))>0),
    source_kind TEXT,
    source_id TEXT,
    source_counterparty_id TEXT,
    source_version TEXT,
    first_application_key TEXT UNIQUE,
    correction_of TEXT REFERENCES fund_allocation_operations(id),
    observation_json TEXT NOT NULL CHECK(json_valid(observation_json) AND json_extract(observation_json,'$.schemaVersion') IS 1),
    source_confirmation_json TEXT NOT NULL CHECK(json_valid(source_confirmation_json) AND json_extract(source_confirmation_json,'$.schemaVersion') IS 1),
    detail_json TEXT NOT NULL CHECK(json_valid(detail_json) AND json_extract(detail_json,'$.schemaVersion') IS 1),
    created_at TEXT NOT NULL,
    created_channel TEXT NOT NULL CHECK(created_channel IN ('gui','cli','mcp')),
    created_actor_kind TEXT NOT NULL CHECK(created_actor_kind IN ('GUI_SESSION','PROGRAM')),
    created_actor_id TEXT NOT NULL,
    created_credential_id TEXT,
    CHECK(source_kind IS NOT 'TRANSFER' OR (source_id IS NOT NULL AND source_counterparty_id IS NOT NULL AND source_id<>source_counterparty_id)),
    CHECK(operation_kind<>'REVERSE' OR correction_of IS NOT NULL)
);
CREATE UNIQUE INDEX fund_operation_one_reversal ON fund_allocation_operations(correction_of) WHERE operation_kind='REVERSE';

CREATE INDEX fund_operation_transfer_source ON fund_allocation_operations(source_id) WHERE source_kind='TRANSFER' AND first_application_key IS NOT NULL;
CREATE INDEX fund_operation_transfer_counterparty ON fund_allocation_operations(source_counterparty_id) WHERE source_kind='TRANSFER' AND first_application_key IS NOT NULL;
CREATE TRIGGER fund_transfer_first_application BEFORE INSERT ON fund_allocation_operations
WHEN NEW.source_kind='TRANSFER' AND NEW.first_application_key IS NOT NULL
 AND EXISTS(SELECT 1 FROM fund_allocation_operations old WHERE old.source_kind='TRANSFER' AND old.first_application_key IS NOT NULL
    AND (old.source_id IN (NEW.source_id,NEW.source_counterparty_id) OR old.source_counterparty_id IN (NEW.source_id,NEW.source_counterparty_id)))
BEGIN SELECT RAISE(ABORT,'transfer ownership already applied'); END;

CREATE TABLE fund_allocation_entries (
    id TEXT PRIMARY KEY NOT NULL,
    operation_id TEXT NOT NULL REFERENCES fund_allocation_operations(id),
    purpose_id TEXT NOT NULL REFERENCES fund_purposes(id),
    cash_delta TEXT CHECK (cash_delta IS NULL OR ((CASE WHEN substr(cash_delta,1,1)='-' THEN substr(cash_delta,2) ELSE cash_delta END) <> '' AND (CASE WHEN substr(cash_delta,1,1)='-' THEN substr(cash_delta,2) ELSE cash_delta END) NOT GLOB '*[^0-9.]*'
        AND (CASE WHEN substr(cash_delta,1,1)='-' THEN substr(cash_delta,2) ELSE cash_delta END) GLOB '[0-9]*' AND (CASE WHEN substr(cash_delta,1,1)='-' THEN substr(cash_delta,2) ELSE cash_delta END) NOT LIKE '%.%.%'
        AND ((CASE WHEN substr(cash_delta,1,1)='-' THEN substr(cash_delta,2) ELSE cash_delta END) = '0' OR substr((CASE WHEN substr(cash_delta,1,1)='-' THEN substr(cash_delta,2) ELSE cash_delta END),1,1) <> '0' OR substr((CASE WHEN substr(cash_delta,1,1)='-' THEN substr(cash_delta,2) ELSE cash_delta END),2,1) = '.')
        AND (instr((CASE WHEN substr(cash_delta,1,1)='-' THEN substr(cash_delta,2) ELSE cash_delta END),'.') = 0 OR (substr((CASE WHEN substr(cash_delta,1,1)='-' THEN substr(cash_delta,2) ELSE cash_delta END),-1) <> '.' AND substr((CASE WHEN substr(cash_delta,1,1)='-' THEN substr(cash_delta,2) ELSE cash_delta END),-1) <> '0')) AND cash_delta <> '-0')),
    quantity_delta TEXT CHECK (quantity_delta IS NULL OR ((CASE WHEN substr(quantity_delta,1,1)='-' THEN substr(quantity_delta,2) ELSE quantity_delta END) <> '' AND (CASE WHEN substr(quantity_delta,1,1)='-' THEN substr(quantity_delta,2) ELSE quantity_delta END) NOT GLOB '*[^0-9.]*'
        AND (CASE WHEN substr(quantity_delta,1,1)='-' THEN substr(quantity_delta,2) ELSE quantity_delta END) GLOB '[0-9]*' AND (CASE WHEN substr(quantity_delta,1,1)='-' THEN substr(quantity_delta,2) ELSE quantity_delta END) NOT LIKE '%.%.%'
        AND ((CASE WHEN substr(quantity_delta,1,1)='-' THEN substr(quantity_delta,2) ELSE quantity_delta END) = '0' OR substr((CASE WHEN substr(quantity_delta,1,1)='-' THEN substr(quantity_delta,2) ELSE quantity_delta END),1,1) <> '0' OR substr((CASE WHEN substr(quantity_delta,1,1)='-' THEN substr(quantity_delta,2) ELSE quantity_delta END),2,1) = '.')
        AND (instr((CASE WHEN substr(quantity_delta,1,1)='-' THEN substr(quantity_delta,2) ELSE quantity_delta END),'.') = 0 OR (substr((CASE WHEN substr(quantity_delta,1,1)='-' THEN substr(quantity_delta,2) ELSE quantity_delta END),-1) <> '.' AND substr((CASE WHEN substr(quantity_delta,1,1)='-' THEN substr(quantity_delta,2) ELSE quantity_delta END),-1) <> '0')) AND quantity_delta <> '-0')),
    created_at TEXT NOT NULL,
    resource_kind TEXT NOT NULL CHECK(resource_kind IN ('CASH','SECURITY')),
    account_id TEXT NOT NULL CHECK(length(trim(account_id)) > 0),
    currency TEXT,
    asset_id TEXT,
    CHECK ((resource_kind='CASH' AND currency IS NOT NULL AND length(currency)=3
            AND currency NOT GLOB '*[^A-Z]*' AND asset_id IS NULL)
        OR (resource_kind='SECURITY' AND currency IS NULL AND asset_id IS NOT NULL AND length(trim(asset_id))>0)),
    CHECK ((resource_kind='CASH' AND cash_delta IS NOT NULL AND quantity_delta IS NULL AND cash_delta<>'0')
        OR (resource_kind='SECURITY' AND quantity_delta IS NOT NULL AND cash_delta IS NULL AND quantity_delta<>'0'))
);
CREATE INDEX fund_entries_operation ON fund_allocation_entries(operation_id);
CREATE INDEX fund_entries_resource ON fund_allocation_entries(account_id,resource_kind,currency,asset_id);

CREATE TABLE fund_allocations (
    id TEXT PRIMARY KEY NOT NULL,
    purpose_id TEXT NOT NULL REFERENCES fund_purposes(id),
    cash_amount TEXT CHECK (cash_amount IS NULL OR (cash_amount <> '' AND cash_amount NOT GLOB '*[^0-9.]*'
        AND cash_amount GLOB '[0-9]*' AND cash_amount NOT LIKE '%.%.%'
        AND (cash_amount = '0' OR substr(cash_amount,1,1) <> '0' OR substr(cash_amount,2,1) = '.')
        AND (instr(cash_amount,'.') = 0 OR (substr(cash_amount,-1) <> '.' AND substr(cash_amount,-1) <> '0')))),
    quantity TEXT CHECK (quantity IS NULL OR (quantity <> '' AND quantity NOT GLOB '*[^0-9.]*'
        AND quantity GLOB '[0-9]*' AND quantity NOT LIKE '%.%.%'
        AND (quantity = '0' OR substr(quantity,1,1) <> '0' OR substr(quantity,2,1) = '.')
        AND (instr(quantity,'.') = 0 OR (substr(quantity,-1) <> '.' AND substr(quantity,-1) <> '0')))),
    last_operation_id TEXT NOT NULL REFERENCES fund_allocation_operations(id),
    resource_kind TEXT NOT NULL CHECK(resource_kind IN ('CASH','SECURITY')),
    account_id TEXT NOT NULL CHECK(length(trim(account_id)) > 0),
    currency TEXT,
    asset_id TEXT,
    CHECK ((resource_kind='CASH' AND currency IS NOT NULL AND length(currency)=3
            AND currency NOT GLOB '*[^A-Z]*' AND asset_id IS NULL)
        OR (resource_kind='SECURITY' AND currency IS NULL AND asset_id IS NOT NULL AND length(trim(asset_id))>0)),
    CHECK ((resource_kind='CASH' AND cash_amount IS NOT NULL AND quantity IS NULL)
        OR (resource_kind='SECURITY' AND quantity IS NOT NULL AND cash_amount IS NULL))
);
CREATE UNIQUE INDEX fund_allocations_cash ON fund_allocations(purpose_id,account_id,currency) WHERE resource_kind='CASH';
CREATE UNIQUE INDEX fund_allocations_security ON fund_allocations(purpose_id,account_id,asset_id) WHERE resource_kind='SECURITY';

CREATE TABLE investment_executions (
    id TEXT PRIMARY KEY NOT NULL,
    source_kind TEXT NOT NULL CHECK(source_kind IN ('ACTIVITY','MANUAL_TRADE')),
    source_id TEXT NOT NULL,
    source_version TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('VALID','RECONCILE','INVALID','DELETED')),
    account_id TEXT NOT NULL,
    asset_id TEXT,
    side TEXT NOT NULL CHECK(side IN ('BUY','SELL')),
    quantity TEXT CHECK (quantity IS NULL OR (quantity <> '' AND quantity NOT GLOB '*[^0-9.]*'
        AND quantity GLOB '[0-9]*' AND quantity NOT LIKE '%.%.%'
        AND (quantity = '0' OR substr(quantity,1,1) <> '0' OR substr(quantity,2,1) = '.')
        AND (instr(quantity,'.') = 0 OR (substr(quantity,-1) <> '.' AND substr(quantity,-1) <> '0')))),
    gross_amount TEXT CHECK (gross_amount IS NULL OR (gross_amount <> '' AND gross_amount NOT GLOB '*[^0-9.]*'
        AND gross_amount GLOB '[0-9]*' AND gross_amount NOT LIKE '%.%.%'
        AND (gross_amount = '0' OR substr(gross_amount,1,1) <> '0' OR substr(gross_amount,2,1) = '.')
        AND (instr(gross_amount,'.') = 0 OR (substr(gross_amount,-1) <> '.' AND substr(gross_amount,-1) <> '0')))),
    activity_currency TEXT NOT NULL,
    net_cash TEXT CHECK (net_cash IS NULL OR ((CASE WHEN substr(net_cash,1,1)='-' THEN substr(net_cash,2) ELSE net_cash END) <> '' AND (CASE WHEN substr(net_cash,1,1)='-' THEN substr(net_cash,2) ELSE net_cash END) NOT GLOB '*[^0-9.]*'
        AND (CASE WHEN substr(net_cash,1,1)='-' THEN substr(net_cash,2) ELSE net_cash END) GLOB '[0-9]*' AND (CASE WHEN substr(net_cash,1,1)='-' THEN substr(net_cash,2) ELSE net_cash END) NOT LIKE '%.%.%'
        AND ((CASE WHEN substr(net_cash,1,1)='-' THEN substr(net_cash,2) ELSE net_cash END) = '0' OR substr((CASE WHEN substr(net_cash,1,1)='-' THEN substr(net_cash,2) ELSE net_cash END),1,1) <> '0' OR substr((CASE WHEN substr(net_cash,1,1)='-' THEN substr(net_cash,2) ELSE net_cash END),2,1) = '.')
        AND (instr((CASE WHEN substr(net_cash,1,1)='-' THEN substr(net_cash,2) ELSE net_cash END),'.') = 0 OR (substr((CASE WHEN substr(net_cash,1,1)='-' THEN substr(net_cash,2) ELSE net_cash END),-1) <> '.' AND substr((CASE WHEN substr(net_cash,1,1)='-' THEN substr(net_cash,2) ELSE net_cash END),-1) <> '0')) AND net_cash <> '-0')),
    settlement_currency TEXT,
    fee TEXT CHECK (fee IS NULL OR (fee <> '' AND fee NOT GLOB '*[^0-9.]*'
        AND fee GLOB '[0-9]*' AND fee NOT LIKE '%.%.%'
        AND (fee = '0' OR substr(fee,1,1) <> '0' OR substr(fee,2,1) = '.')
        AND (instr(fee,'.') = 0 OR (substr(fee,-1) <> '.' AND substr(fee,-1) <> '0')))),
    tax TEXT CHECK (tax IS NULL OR (tax <> '' AND tax NOT GLOB '*[^0-9.]*'
        AND tax GLOB '[0-9]*' AND tax NOT LIKE '%.%.%'
        AND (tax = '0' OR substr(tax,1,1) <> '0' OR substr(tax,2,1) = '.')
        AND (instr(tax,'.') = 0 OR (substr(tax,-1) <> '.' AND substr(tax,-1) <> '0')))),
    cash_basis TEXT NOT NULL,
    occurred_at TEXT NOT NULL,
    time_precision TEXT NOT NULL CHECK(time_precision IN ('DATE','DATETIME')),
    snapshot_json TEXT NOT NULL CHECK(json_valid(snapshot_json) AND json_extract(snapshot_json,'$.schemaVersion') IS 1),
    UNIQUE(source_kind,source_id)
);
CREATE INDEX investment_executions_date ON investment_executions(occurred_at,status);

CREATE TABLE investment_projection_state (
    source_kind TEXT NOT NULL CHECK(source_kind IN ('ACTIVITY','MANUAL_TRADE','SNAPSHOT')),
    source_id TEXT NOT NULL,
    generation INTEGER NOT NULL DEFAULT 1 CHECK(generation>0),
    processed_generation INTEGER NOT NULL DEFAULT 0 CHECK(processed_generation>=0 AND processed_generation<=generation),
    refresh_target_generation INTEGER,
    change_kind TEXT NOT NULL CHECK(change_kind IN ('INSERT','UPDATE','DELETE','BACKFILL')),
    is_deleted INTEGER NOT NULL DEFAULT 0 CHECK(is_deleted IN (0,1)),
    source_snapshot_json TEXT NOT NULL CHECK(json_valid(source_snapshot_json)),
    resource_impact_json TEXT CHECK(resource_impact_json IS NULL OR json_valid(resource_impact_json)),
    PRIMARY KEY(source_kind,source_id)
);
CREATE INDEX investment_projection_pending ON investment_projection_state(processed_generation,generation);

CREATE TRIGGER fund_allocation_operations_no_update BEFORE UPDATE ON fund_allocation_operations
BEGIN SELECT RAISE(ABORT,'investment ownership history is append-only'); END;

CREATE TRIGGER fund_allocation_operations_no_delete BEFORE DELETE ON fund_allocation_operations
BEGIN SELECT RAISE(ABORT,'investment ownership history is append-only'); END;

CREATE TRIGGER fund_allocation_entries_no_update BEFORE UPDATE ON fund_allocation_entries
BEGIN SELECT RAISE(ABORT,'investment ownership history is append-only'); END;

CREATE TRIGGER fund_allocation_entries_no_delete BEFORE DELETE ON fund_allocation_entries
BEGIN SELECT RAISE(ABORT,'investment ownership history is append-only'); END;

CREATE TRIGGER investment_source_activities_insert AFTER INSERT ON activities
WHEN 1
BEGIN
    INSERT INTO investment_projection_state
        (source_kind,source_id,generation,processed_generation,change_kind,is_deleted,source_snapshot_json)
    VALUES ('ACTIVITY',NEW.id,1,0,'INSERT',0,
        json_object('schemaVersion',1,'before',NULL,'after',json_object('schemaVersion',1,'id',NEW.id,'account_id',NEW.account_id,'asset_id',NEW.asset_id,'activity_type',NEW.activity_type,'activity_type_override',NEW.activity_type_override,'subtype',NEW.subtype,'status',NEW.status,'activity_date',NEW.activity_date,'settlement_date',NEW.settlement_date,'quantity',NEW.quantity,'unit_price',NEW.unit_price,'amount',NEW.amount,'fee',NEW.fee,'tax',NEW.tax,'currency',NEW.currency,'fx_rate',NEW.fx_rate,'metadata',NEW.metadata,'source_group_id',NEW.source_group_id,'source_system',NEW.source_system,'is_user_modified',NEW.is_user_modified,'created_at',NEW.created_at,'updated_at',NEW.updated_at)))
    ON CONFLICT(source_kind,source_id) DO UPDATE SET
        generation=investment_projection_state.generation+1,
        change_kind=excluded.change_kind,
        is_deleted=excluded.is_deleted,
        source_snapshot_json=excluded.source_snapshot_json;
    -- An accepted trade can change when related facts change (for example,
    -- a late purchase makes an old disposal match). Reproject; never replay
    -- that trade's ownership entries.
    UPDATE investment_projection_state SET generation=generation+1,change_kind='UPDATE'
    WHERE source_kind='ACTIVITY' AND source_id<>NEW.id AND is_deleted=0
      AND EXISTS(SELECT 1 FROM activities a WHERE a.id=investment_projection_state.source_id
        AND UPPER(COALESCE(NULLIF(TRIM(a.activity_type_override, char(9, 10, 11, 12, 13, 32, 133, 160, 5760, 8192, 8193, 8194, 8195, 8196, 8197, 8198, 8199, 8200, 8201, 8202, 8232, 8233, 8239, 8287, 12288)),''),a.activity_type)) IN ('BUY','SELL')
        AND (a.account_id IN (NEW.account_id) OR a.asset_id IN (NEW.asset_id)));

END;

CREATE TRIGGER investment_source_activities_update AFTER UPDATE OF id,account_id,asset_id,activity_type,activity_type_override,subtype,status,activity_date,settlement_date,quantity,unit_price,amount,fee,tax,currency,fx_rate,metadata,source_group_id,source_system,is_user_modified,created_at,updated_at ON activities
WHEN (1 OR 1) AND (NEW.id IS NOT OLD.id OR NEW.account_id IS NOT OLD.account_id OR NEW.asset_id IS NOT OLD.asset_id OR NEW.activity_type IS NOT OLD.activity_type OR NEW.activity_type_override IS NOT OLD.activity_type_override OR NEW.subtype IS NOT OLD.subtype OR NEW.status IS NOT OLD.status OR NEW.activity_date IS NOT OLD.activity_date OR NEW.settlement_date IS NOT OLD.settlement_date OR NEW.quantity IS NOT OLD.quantity OR NEW.unit_price IS NOT OLD.unit_price OR NEW.amount IS NOT OLD.amount OR NEW.fee IS NOT OLD.fee OR NEW.tax IS NOT OLD.tax OR NEW.currency IS NOT OLD.currency OR NEW.fx_rate IS NOT OLD.fx_rate OR NEW.metadata IS NOT OLD.metadata OR NEW.source_group_id IS NOT OLD.source_group_id OR NEW.source_system IS NOT OLD.source_system OR NEW.is_user_modified IS NOT OLD.is_user_modified OR NEW.created_at IS NOT OLD.created_at OR NEW.updated_at IS NOT OLD.updated_at)
BEGIN
    INSERT INTO investment_projection_state
        (source_kind,source_id,generation,processed_generation,change_kind,is_deleted,source_snapshot_json)
    VALUES ('ACTIVITY',NEW.id,1,0,'UPDATE',0,
        json_object('schemaVersion',1,'before',json_object('schemaVersion',1,'id',OLD.id,'account_id',OLD.account_id,'asset_id',OLD.asset_id,'activity_type',OLD.activity_type,'activity_type_override',OLD.activity_type_override,'subtype',OLD.subtype,'status',OLD.status,'activity_date',OLD.activity_date,'settlement_date',OLD.settlement_date,'quantity',OLD.quantity,'unit_price',OLD.unit_price,'amount',OLD.amount,'fee',OLD.fee,'tax',OLD.tax,'currency',OLD.currency,'fx_rate',OLD.fx_rate,'metadata',OLD.metadata,'source_group_id',OLD.source_group_id,'source_system',OLD.source_system,'is_user_modified',OLD.is_user_modified,'created_at',OLD.created_at,'updated_at',OLD.updated_at),'after',json_object('schemaVersion',1,'id',NEW.id,'account_id',NEW.account_id,'asset_id',NEW.asset_id,'activity_type',NEW.activity_type,'activity_type_override',NEW.activity_type_override,'subtype',NEW.subtype,'status',NEW.status,'activity_date',NEW.activity_date,'settlement_date',NEW.settlement_date,'quantity',NEW.quantity,'unit_price',NEW.unit_price,'amount',NEW.amount,'fee',NEW.fee,'tax',NEW.tax,'currency',NEW.currency,'fx_rate',NEW.fx_rate,'metadata',NEW.metadata,'source_group_id',NEW.source_group_id,'source_system',NEW.source_system,'is_user_modified',NEW.is_user_modified,'created_at',NEW.created_at,'updated_at',NEW.updated_at)))
    ON CONFLICT(source_kind,source_id) DO UPDATE SET
        generation=investment_projection_state.generation+1,
        change_kind=excluded.change_kind,
        is_deleted=excluded.is_deleted,
        source_snapshot_json=excluded.source_snapshot_json;
    -- An accepted trade can change when related facts change (for example,
    -- a late purchase makes an old disposal match). Reproject; never replay
    -- that trade's ownership entries.
    UPDATE investment_projection_state SET generation=generation+1,change_kind='UPDATE'
    WHERE source_kind='ACTIVITY' AND source_id<>NEW.id AND is_deleted=0
      AND EXISTS(SELECT 1 FROM activities a WHERE a.id=investment_projection_state.source_id
        AND UPPER(COALESCE(NULLIF(TRIM(a.activity_type_override, char(9, 10, 11, 12, 13, 32, 133, 160, 5760, 8192, 8193, 8194, 8195, 8196, 8197, 8198, 8199, 8200, 8201, 8202, 8232, 8233, 8239, 8287, 12288)),''),a.activity_type)) IN ('BUY','SELL')
        AND (a.account_id IN (OLD.account_id,NEW.account_id) OR a.asset_id IN (OLD.asset_id,NEW.asset_id)));

END;

CREATE TRIGGER investment_source_activities_delete AFTER DELETE ON activities
WHEN 1
BEGIN
    INSERT INTO investment_projection_state
        (source_kind,source_id,generation,processed_generation,change_kind,is_deleted,source_snapshot_json)
    VALUES ('ACTIVITY',OLD.id,1,0,'DELETE',1,
        json_object('schemaVersion',1,'before',json_object('schemaVersion',1,'id',OLD.id,'account_id',OLD.account_id,'asset_id',OLD.asset_id,'activity_type',OLD.activity_type,'activity_type_override',OLD.activity_type_override,'subtype',OLD.subtype,'status',OLD.status,'activity_date',OLD.activity_date,'settlement_date',OLD.settlement_date,'quantity',OLD.quantity,'unit_price',OLD.unit_price,'amount',OLD.amount,'fee',OLD.fee,'tax',OLD.tax,'currency',OLD.currency,'fx_rate',OLD.fx_rate,'metadata',OLD.metadata,'source_group_id',OLD.source_group_id,'source_system',OLD.source_system,'is_user_modified',OLD.is_user_modified,'created_at',OLD.created_at,'updated_at',OLD.updated_at),'after',NULL))
    ON CONFLICT(source_kind,source_id) DO UPDATE SET
        generation=investment_projection_state.generation+1,
        change_kind=excluded.change_kind,
        is_deleted=excluded.is_deleted,
        source_snapshot_json=excluded.source_snapshot_json;
    -- An accepted trade can change when related facts change (for example,
    -- a late purchase makes an old disposal match). Reproject; never replay
    -- that trade's ownership entries.
    UPDATE investment_projection_state SET generation=generation+1,change_kind='UPDATE'
    WHERE source_kind='ACTIVITY' AND source_id<>OLD.id AND is_deleted=0
      AND EXISTS(SELECT 1 FROM activities a WHERE a.id=investment_projection_state.source_id
        AND UPPER(COALESCE(NULLIF(TRIM(a.activity_type_override, char(9, 10, 11, 12, 13, 32, 133, 160, 5760, 8192, 8193, 8194, 8195, 8196, 8197, 8198, 8199, 8200, 8201, 8202, 8232, 8233, 8239, 8287, 12288)),''),a.activity_type)) IN ('BUY','SELL')
        AND (a.account_id IN (OLD.account_id) OR a.asset_id IN (OLD.asset_id)));

END;

CREATE TRIGGER investment_source_business_requests_insert AFTER INSERT ON business_requests
WHEN NEW.operation IN ('manual_trade_record','investment_manual_trade_record') AND NEW.state='completed'
BEGIN
    INSERT INTO investment_projection_state
        (source_kind,source_id,generation,processed_generation,change_kind,is_deleted,source_snapshot_json)
    VALUES ('MANUAL_TRADE',NEW.id,1,0,'INSERT',0,
        json_object('schemaVersion',1,'before',NULL,'after',json_object('schemaVersion',1,'id',NEW.id,'operation',NEW.operation,'state',NEW.state,'request_hash',NEW.request_hash,'request_snapshot',NEW.request_snapshot,'result_json',NEW.result_json)))
    ON CONFLICT(source_kind,source_id) DO UPDATE SET
        generation=investment_projection_state.generation+1,
        change_kind=excluded.change_kind,
        is_deleted=excluded.is_deleted,
        source_snapshot_json=excluded.source_snapshot_json;
END;

CREATE TRIGGER investment_source_business_requests_update AFTER UPDATE OF id,operation,state,request_hash,request_snapshot,result_json ON business_requests
WHEN (OLD.operation IN ('manual_trade_record','investment_manual_trade_record') AND OLD.state='completed' OR NEW.operation IN ('manual_trade_record','investment_manual_trade_record') AND NEW.state='completed') AND (NEW.id IS NOT OLD.id OR NEW.operation IS NOT OLD.operation OR NEW.state IS NOT OLD.state OR NEW.request_hash IS NOT OLD.request_hash OR NEW.request_snapshot IS NOT OLD.request_snapshot OR NEW.result_json IS NOT OLD.result_json)
BEGIN
    INSERT INTO investment_projection_state
        (source_kind,source_id,generation,processed_generation,change_kind,is_deleted,source_snapshot_json)
    VALUES ('MANUAL_TRADE',NEW.id,1,0,'UPDATE',0,
        json_object('schemaVersion',1,'before',json_object('schemaVersion',1,'id',OLD.id,'operation',OLD.operation,'state',OLD.state,'request_hash',OLD.request_hash,'request_snapshot',OLD.request_snapshot,'result_json',OLD.result_json),'after',json_object('schemaVersion',1,'id',NEW.id,'operation',NEW.operation,'state',NEW.state,'request_hash',NEW.request_hash,'request_snapshot',NEW.request_snapshot,'result_json',NEW.result_json)))
    ON CONFLICT(source_kind,source_id) DO UPDATE SET
        generation=investment_projection_state.generation+1,
        change_kind=excluded.change_kind,
        is_deleted=excluded.is_deleted,
        source_snapshot_json=excluded.source_snapshot_json;
END;

CREATE TRIGGER investment_source_business_requests_delete AFTER DELETE ON business_requests
WHEN (OLD.operation IN ('manual_trade_record','investment_manual_trade_record') AND OLD.state='completed') OR EXISTS(SELECT 1 FROM investment_projection_state s WHERE s.source_kind='MANUAL_TRADE' AND s.source_id=OLD.id)
BEGIN
    INSERT INTO investment_projection_state
        (source_kind,source_id,generation,processed_generation,change_kind,is_deleted,source_snapshot_json)
    VALUES ('MANUAL_TRADE',OLD.id,1,0,'DELETE',1,
        json_object('schemaVersion',1,'before',json_object('schemaVersion',1,'id',OLD.id,'operation',OLD.operation,'state',OLD.state,'request_hash',OLD.request_hash,'request_snapshot',OLD.request_snapshot,'result_json',OLD.result_json),'after',NULL))
    ON CONFLICT(source_kind,source_id) DO UPDATE SET
        generation=investment_projection_state.generation+1,
        change_kind=excluded.change_kind,
        is_deleted=excluded.is_deleted,
        source_snapshot_json=excluded.source_snapshot_json;
END;

CREATE TRIGGER investment_source_holdings_snapshots_insert AFTER INSERT ON holdings_snapshots
WHEN NEW.source<>'CALCULATED'
BEGIN
    INSERT INTO investment_projection_state
        (source_kind,source_id,generation,processed_generation,change_kind,is_deleted,source_snapshot_json)
    VALUES ('SNAPSHOT',NEW.id,1,0,'INSERT',0,
        json_object('schemaVersion',1,'before',NULL,'after',json_object('schemaVersion',1,'id',NEW.id,'account_id',NEW.account_id,'snapshot_date',NEW.snapshot_date,'source',NEW.source,'currency',NEW.currency,'positions',NEW.positions,'cash_balances',NEW.cash_balances)))
    ON CONFLICT(source_kind,source_id) DO UPDATE SET
        generation=investment_projection_state.generation+1,
        change_kind=excluded.change_kind,
        is_deleted=excluded.is_deleted,
        source_snapshot_json=excluded.source_snapshot_json;
END;

CREATE TRIGGER investment_source_holdings_snapshots_update AFTER UPDATE OF id,account_id,snapshot_date,source,currency,positions,cash_balances ON holdings_snapshots
WHEN (OLD.source<>'CALCULATED' OR NEW.source<>'CALCULATED') AND (NEW.id IS NOT OLD.id OR NEW.account_id IS NOT OLD.account_id OR NEW.snapshot_date IS NOT OLD.snapshot_date OR NEW.source IS NOT OLD.source OR NEW.currency IS NOT OLD.currency OR NEW.positions IS NOT OLD.positions OR NEW.cash_balances IS NOT OLD.cash_balances)
BEGIN
    INSERT INTO investment_projection_state
        (source_kind,source_id,generation,processed_generation,change_kind,is_deleted,source_snapshot_json)
    VALUES ('SNAPSHOT',NEW.id,1,0,'UPDATE',0,
        json_object('schemaVersion',1,'before',json_object('schemaVersion',1,'id',OLD.id,'account_id',OLD.account_id,'snapshot_date',OLD.snapshot_date,'source',OLD.source,'currency',OLD.currency,'positions',OLD.positions,'cash_balances',OLD.cash_balances),'after',json_object('schemaVersion',1,'id',NEW.id,'account_id',NEW.account_id,'snapshot_date',NEW.snapshot_date,'source',NEW.source,'currency',NEW.currency,'positions',NEW.positions,'cash_balances',NEW.cash_balances)))
    ON CONFLICT(source_kind,source_id) DO UPDATE SET
        generation=investment_projection_state.generation+1,
        change_kind=excluded.change_kind,
        is_deleted=excluded.is_deleted,
        source_snapshot_json=excluded.source_snapshot_json;
END;

CREATE TRIGGER investment_source_holdings_snapshots_delete AFTER DELETE ON holdings_snapshots
WHEN (OLD.source<>'CALCULATED') OR EXISTS(SELECT 1 FROM investment_projection_state s WHERE s.source_kind='SNAPSHOT' AND s.source_id=OLD.id)
BEGIN
    INSERT INTO investment_projection_state
        (source_kind,source_id,generation,processed_generation,change_kind,is_deleted,source_snapshot_json)
    VALUES ('SNAPSHOT',OLD.id,1,0,'DELETE',1,
        json_object('schemaVersion',1,'before',json_object('schemaVersion',1,'id',OLD.id,'account_id',OLD.account_id,'snapshot_date',OLD.snapshot_date,'source',OLD.source,'currency',OLD.currency,'positions',OLD.positions,'cash_balances',OLD.cash_balances),'after',NULL))
    ON CONFLICT(source_kind,source_id) DO UPDATE SET
        generation=investment_projection_state.generation+1,
        change_kind=excluded.change_kind,
        is_deleted=excluded.is_deleted,
        source_snapshot_json=excluded.source_snapshot_json;
END;

-- Account mode/eligibility and instrument semantics are accounting facts too.
-- Market quotes and derived snapshot refreshes do not invalidate executions.
CREATE TRIGGER investment_dependencies_accounts_update
AFTER UPDATE OF currency,account_type,is_active,is_archived,tracking_mode,meta ON accounts
WHEN NEW.currency IS NOT OLD.currency OR NEW.account_type IS NOT OLD.account_type
  OR NEW.is_active IS NOT OLD.is_active OR NEW.is_archived IS NOT OLD.is_archived
  OR NEW.tracking_mode IS NOT OLD.tracking_mode OR NEW.meta IS NOT OLD.meta
BEGIN
    UPDATE investment_projection_state SET generation=generation+1,change_kind='UPDATE'
    WHERE source_kind='ACTIVITY' AND is_deleted=0
      AND EXISTS(SELECT 1 FROM activities a WHERE a.id=investment_projection_state.source_id
        AND a.account_id=NEW.id
        AND UPPER(COALESCE(NULLIF(TRIM(a.activity_type_override, char(9, 10, 11, 12, 13, 32, 133, 160, 5760, 8192, 8193, 8194, 8195, 8196, 8197, 8198, 8199, 8200, 8201, 8202, 8232, 8233, 8239, 8287, 12288)),''),a.activity_type)) IN ('BUY','SELL'));
END;

CREATE TRIGGER investment_dependencies_assets_update
AFTER UPDATE OF kind,quote_ccy,instrument_type,metadata ON assets
WHEN NEW.kind IS NOT OLD.kind OR NEW.quote_ccy IS NOT OLD.quote_ccy
  OR NEW.instrument_type IS NOT OLD.instrument_type OR NEW.metadata IS NOT OLD.metadata
BEGIN
    UPDATE investment_projection_state SET generation=generation+1,change_kind='UPDATE'
    WHERE source_kind='ACTIVITY' AND is_deleted=0
      AND EXISTS(SELECT 1 FROM activities a WHERE a.id=investment_projection_state.source_id
        AND a.asset_id=NEW.id
        AND UPPER(COALESCE(NULLIF(TRIM(a.activity_type_override, char(9, 10, 11, 12, 13, 32, 133, 160, 5760, 8192, 8193, 8194, 8195, 8196, 8197, 8198, 8199, 8200, 8201, 8202, 8232, 8233, 8239, 8287, 12288)),''),a.activity_type)) IN ('BUY','SELL'));
END;
