-- TransitGuard authoritative PostgreSQL schema for accounts, credentials,
-- optimistic-concurrency versions, and immutable domain events.

CREATE TABLE transit_accounts (
    id UUID PRIMARY KEY,
    rider_id UUID NOT NULL UNIQUE,
    status TEXT NOT NULL,
    eligibility TEXT NOT NULL,
    stored_value_minor_units BIGINT NOT NULL,
    stored_value_currency CHAR(3) NOT NULL,
    aggregate_version BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,

    CONSTRAINT transit_accounts_id_not_nil_check
        CHECK (id <> '00000000-0000-0000-0000-000000000000'::UUID),

    CONSTRAINT transit_accounts_id_version_check
        CHECK (SUBSTRING(id::TEXT FROM 15 FOR 1) = '7'),

    CONSTRAINT transit_accounts_id_variant_check
        CHECK (
            SUBSTRING(id::TEXT FROM 20 FOR 1)
            IN ('8', '9', 'a', 'b')
        ),

    CONSTRAINT transit_accounts_rider_id_not_nil_check
        CHECK (
            rider_id
            <> '00000000-0000-0000-0000-000000000000'::UUID
        ),

    CONSTRAINT transit_accounts_rider_id_version_check
        CHECK (SUBSTRING(rider_id::TEXT FROM 15 FOR 1) = '7'),

    CONSTRAINT transit_accounts_rider_id_variant_check
        CHECK (
            SUBSTRING(rider_id::TEXT FROM 20 FOR 1)
            IN ('8', '9', 'a', 'b')
        ),

    CONSTRAINT transit_accounts_status_check
        CHECK (
            status IN (
                'active',
                'suspended',
                'closed'
            )
        ),

    CONSTRAINT transit_accounts_eligibility_check
        CHECK (
            eligibility IN (
                'standard',
                'youth',
                'senior',
                'reduced_fare',
                'employee_test_account'
            )
        ),

    CONSTRAINT transit_accounts_balance_check
        CHECK (stored_value_minor_units >= 0),

    CONSTRAINT transit_accounts_currency_check
        CHECK (
            stored_value_currency IN (
                'USD',
                'CAD',
                'EUR',
                'GBP'
            )
        ),

    CONSTRAINT transit_accounts_version_check
        CHECK (aggregate_version > 0)
);

CREATE INDEX transit_accounts_status_index
    ON transit_accounts (status);

CREATE TABLE fare_credentials (
    id UUID PRIMARY KEY,
    transit_account_id UUID NOT NULL,
    kind TEXT NOT NULL,
    status TEXT NOT NULL,
    revocation_reason TEXT,
    replacement_id UUID,
    aggregate_version BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,

    CONSTRAINT fare_credentials_account_fk
        FOREIGN KEY (transit_account_id)
        REFERENCES transit_accounts (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    CONSTRAINT fare_credentials_replacement_fk
        FOREIGN KEY (replacement_id)
        REFERENCES fare_credentials (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT
        DEFERRABLE INITIALLY DEFERRED,

    CONSTRAINT fare_credentials_replacement_unique
        UNIQUE (replacement_id),

    CONSTRAINT fare_credentials_id_not_nil_check
        CHECK (id <> '00000000-0000-0000-0000-000000000000'::UUID),

    CONSTRAINT fare_credentials_id_version_check
        CHECK (SUBSTRING(id::TEXT FROM 15 FOR 1) = '7'),

    CONSTRAINT fare_credentials_id_variant_check
        CHECK (
            SUBSTRING(id::TEXT FROM 20 FOR 1)
            IN ('8', '9', 'a', 'b')
        ),

    CONSTRAINT fare_credentials_kind_check
        CHECK (
            kind IN (
                'card',
                'mobile',
                'development_test_token'
            )
        ),

    CONSTRAINT fare_credentials_status_check
        CHECK (
            status IN (
                'pending',
                'active',
                'suspended',
                'revoked',
                'expired',
                'replaced'
            )
        ),

    CONSTRAINT fare_credentials_revocation_reason_check
        CHECK (
            revocation_reason IS NULL
            OR revocation_reason IN (
                'reported_lost',
                'reported_stolen',
                'replaced',
                'account_suspended',
                'administrative_action',
                'security_incident',
                'test_cleanup'
            )
        ),

    CONSTRAINT fare_credentials_revocation_state_check
        CHECK (
            (
                status = 'revoked'
                AND revocation_reason IS NOT NULL
            )
            OR (
                status <> 'revoked'
                AND revocation_reason IS NULL
            )
        ),

    CONSTRAINT fare_credentials_replacement_state_check
        CHECK (
            (
                status = 'replaced'
                AND replacement_id IS NOT NULL
            )
            OR (
                status <> 'replaced'
                AND replacement_id IS NULL
            )
        ),

    CONSTRAINT fare_credentials_no_self_replacement_check
        CHECK (
            replacement_id IS NULL
            OR replacement_id <> id
        ),

    CONSTRAINT fare_credentials_version_check
        CHECK (aggregate_version > 0)
);

CREATE INDEX fare_credentials_account_index
    ON fare_credentials (transit_account_id);

CREATE INDEX fare_credentials_status_index
    ON fare_credentials (status);

CREATE TABLE domain_events (
    id UUID PRIMARY KEY,
    aggregate_kind TEXT NOT NULL,
    aggregate_id UUID NOT NULL,
    aggregate_version BIGINT NOT NULL,
    event_name TEXT NOT NULL,
    occurred_at_unix_ms BIGINT NOT NULL,
    payload JSONB NOT NULL,
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,

    CONSTRAINT domain_events_id_not_nil_check
        CHECK (id <> '00000000-0000-0000-0000-000000000000'::UUID),

    CONSTRAINT domain_events_id_version_check
        CHECK (SUBSTRING(id::TEXT FROM 15 FOR 1) = '7'),

    CONSTRAINT domain_events_id_variant_check
        CHECK (
            SUBSTRING(id::TEXT FROM 20 FOR 1)
            IN ('8', '9', 'a', 'b')
        ),

    CONSTRAINT domain_events_aggregate_id_not_nil_check
        CHECK (
            aggregate_id
            <> '00000000-0000-0000-0000-000000000000'::UUID
        ),

    CONSTRAINT domain_events_aggregate_id_version_check
        CHECK (
            SUBSTRING(aggregate_id::TEXT FROM 15 FOR 1) = '7'
        ),

    CONSTRAINT domain_events_aggregate_id_variant_check
        CHECK (
            SUBSTRING(aggregate_id::TEXT FROM 20 FOR 1)
            IN ('8', '9', 'a', 'b')
        ),

    CONSTRAINT domain_events_aggregate_kind_check
        CHECK (
            aggregate_kind IN (
                'transit_account',
                'fare_credential',
                'reader_equipment',
                'fare_transaction'
            )
        ),

    CONSTRAINT domain_events_version_check
        CHECK (aggregate_version > 0),

    CONSTRAINT domain_events_occurred_at_check
        CHECK (occurred_at_unix_ms >= 0),

    CONSTRAINT domain_events_payload_check
        CHECK (JSONB_TYPEOF(payload) = 'object'),

    CONSTRAINT domain_events_name_check
        CHECK (
            event_name <> ''
            AND event_name LIKE aggregate_kind || '.%'
        ),

    CONSTRAINT domain_events_aggregate_version_unique
        UNIQUE (
            aggregate_kind,
            aggregate_id,
            aggregate_version
        )
);

CREATE INDEX domain_events_occurred_at_index
    ON domain_events (occurred_at_unix_ms);

CREATE INDEX domain_events_event_name_index
    ON domain_events (event_name);
