-- TransitGuard durable reconciliation work queue.
--
-- This table coordinates bounded backend reconciliation workers without
-- mutating synchronized reader transaction history.
--
-- Claims use renewable database-backed leases. If a worker terminates before
-- completing its claim, the expired lease can be returned to pending state and
-- safely retried.

CREATE TABLE reconciliation_work_items (
    fare_transaction_id UUID PRIMARY KEY,

    reader_id UUID NOT NULL,

    source_batch_id UUID NOT NULL,

    state TEXT NOT NULL
        DEFAULT 'pending',

    attempt_count INTEGER NOT NULL
        DEFAULT 0,

    available_at_unix_milliseconds BIGINT NOT NULL,

    lease_owner_id UUID,

    claimed_at_unix_milliseconds BIGINT,

    lease_expires_at_unix_milliseconds BIGINT,

    completed_at_unix_milliseconds BIGINT,

    created_at TIMESTAMPTZ NOT NULL
        DEFAULT CURRENT_TIMESTAMP,

    updated_at TIMESTAMPTZ NOT NULL
        DEFAULT CURRENT_TIMESTAMP,

    CONSTRAINT reconciliation_work_items_transaction_not_nil
        CHECK (
            fare_transaction_id
            <> '00000000-0000-0000-0000-000000000000'::UUID
        ),

    CONSTRAINT reconciliation_work_items_transaction_version
        CHECK (
            SUBSTRING(
                fare_transaction_id::TEXT
                FROM 15 FOR 1
            ) = '7'
        ),

    CONSTRAINT reconciliation_work_items_transaction_variant
        CHECK (
            SUBSTRING(
                fare_transaction_id::TEXT
                FROM 20 FOR 1
            ) IN ('8', '9', 'a', 'b')
        ),

    CONSTRAINT reconciliation_work_items_reader_not_nil
        CHECK (
            reader_id
            <> '00000000-0000-0000-0000-000000000000'::UUID
        ),

    CONSTRAINT reconciliation_work_items_reader_version
        CHECK (
            SUBSTRING(
                reader_id::TEXT
                FROM 15 FOR 1
            ) = '7'
        ),

    CONSTRAINT reconciliation_work_items_reader_variant
        CHECK (
            SUBSTRING(
                reader_id::TEXT
                FROM 20 FOR 1
            ) IN ('8', '9', 'a', 'b')
        ),

    CONSTRAINT reconciliation_work_items_batch_not_nil
        CHECK (
            source_batch_id
            <> '00000000-0000-0000-0000-000000000000'::UUID
        ),

    CONSTRAINT reconciliation_work_items_batch_version
        CHECK (
            SUBSTRING(
                source_batch_id::TEXT
                FROM 15 FOR 1
            ) = '7'
        ),

    CONSTRAINT reconciliation_work_items_batch_variant
        CHECK (
            SUBSTRING(
                source_batch_id::TEXT
                FROM 20 FOR 1
            ) IN ('8', '9', 'a', 'b')
        ),

    CONSTRAINT reconciliation_work_items_state
        CHECK (
            state IN (
                'pending',
                'in_progress',
                'completed'
            )
        ),

    CONSTRAINT reconciliation_work_items_attempt_count
        CHECK (
            attempt_count >= 0
        ),

    CONSTRAINT reconciliation_work_items_available_time
        CHECK (
            available_at_unix_milliseconds >= 0
        ),

    CONSTRAINT reconciliation_work_items_lease_owner
        CHECK (
            lease_owner_id IS NULL
            OR (
                lease_owner_id
                <> '00000000-0000-0000-0000-000000000000'::UUID
                AND SUBSTRING(
                    lease_owner_id::TEXT
                    FROM 15 FOR 1
                ) = '7'
                AND SUBSTRING(
                    lease_owner_id::TEXT
                    FROM 20 FOR 1
                ) IN ('8', '9', 'a', 'b')
            )
        ),

    CONSTRAINT reconciliation_work_items_claim_time
        CHECK (
            claimed_at_unix_milliseconds IS NULL
            OR claimed_at_unix_milliseconds >= 0
        ),

    CONSTRAINT reconciliation_work_items_lease_time
        CHECK (
            lease_expires_at_unix_milliseconds IS NULL
            OR lease_expires_at_unix_milliseconds >= 0
        ),

    CONSTRAINT reconciliation_work_items_completed_time
        CHECK (
            completed_at_unix_milliseconds IS NULL
            OR completed_at_unix_milliseconds >= 0
        ),

    CONSTRAINT reconciliation_work_items_lease_range
        CHECK (
            lease_expires_at_unix_milliseconds IS NULL
            OR (
                claimed_at_unix_milliseconds IS NOT NULL
                AND lease_expires_at_unix_milliseconds
                    > claimed_at_unix_milliseconds
            )
        ),

    CONSTRAINT reconciliation_work_items_state_metadata
        CHECK (
            (
                state = 'pending'
                AND lease_owner_id IS NULL
                AND claimed_at_unix_milliseconds IS NULL
                AND lease_expires_at_unix_milliseconds IS NULL
                AND completed_at_unix_milliseconds IS NULL
            )
            OR (
                state = 'in_progress'
                AND attempt_count > 0
                AND lease_owner_id IS NOT NULL
                AND claimed_at_unix_milliseconds IS NOT NULL
                AND lease_expires_at_unix_milliseconds IS NOT NULL
                AND completed_at_unix_milliseconds IS NULL
            )
            OR (
                state = 'completed'
                AND attempt_count > 0
                AND lease_owner_id IS NULL
                AND claimed_at_unix_milliseconds IS NULL
                AND lease_expires_at_unix_milliseconds IS NULL
                AND completed_at_unix_milliseconds IS NOT NULL
            )
        ),

    CONSTRAINT reconciliation_work_items_updated_time
        CHECK (
            updated_at >= created_at
        ),

    CONSTRAINT reconciliation_work_items_transaction_fk
        FOREIGN KEY (fare_transaction_id)
        REFERENCES synchronization_ingest_transactions (
            fare_transaction_id
        )
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    CONSTRAINT reconciliation_work_items_reader_fk
        FOREIGN KEY (reader_id)
        REFERENCES reader_equipment (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    CONSTRAINT reconciliation_work_items_batch_reader_fk
        FOREIGN KEY (
            source_batch_id,
            reader_id
        )
        REFERENCES synchronization_ingest_batches (
            batch_id,
            reader_id
        )
        ON UPDATE RESTRICT
        ON DELETE RESTRICT
);

CREATE INDEX reconciliation_work_items_pending_index
    ON reconciliation_work_items (
        available_at_unix_milliseconds,
        fare_transaction_id
    )
    WHERE state = 'pending';

CREATE INDEX reconciliation_work_items_expired_lease_index
    ON reconciliation_work_items (
        lease_expires_at_unix_milliseconds,
        fare_transaction_id
    )
    WHERE state = 'in_progress';

CREATE INDEX reconciliation_work_items_reader_index
    ON reconciliation_work_items (
        reader_id,
        state,
        available_at_unix_milliseconds
    );
