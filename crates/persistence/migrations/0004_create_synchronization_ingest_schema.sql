-- TransitGuard PostgreSQL storage for project-owned reader synchronization.
--
-- These tables preserve immutable received-batch history, stable transaction
-- identities, per-entry outcomes, canonical protocol evidence, and replay
-- fingerprints.
--
-- They never store real transit-authority messages, payment-card data,
-- private signing keys, authentication tokens, or production credentials.

CREATE TABLE synchronization_ingest_batches (
    batch_id UUID PRIMARY KEY,

    reader_id UUID NOT NULL,

    protocol_version INTEGER NOT NULL,

    environment_id TEXT NOT NULL,

    reader_software_version TEXT NOT NULL,

    first_local_sequence_number BIGINT NOT NULL,

    last_local_sequence_number BIGINT NOT NULL,

    submitted_at_unix_milliseconds BIGINT NOT NULL,

    received_at_unix_milliseconds BIGINT NOT NULL,

    entry_count INTEGER NOT NULL,

    request_fingerprint TEXT NOT NULL,

    canonical_request_json JSONB NOT NULL,

    acknowledgement_fingerprint TEXT NOT NULL,

    canonical_acknowledgement_json JSONB NOT NULL,

    created_at TIMESTAMPTZ NOT NULL
        DEFAULT CURRENT_TIMESTAMP,

    CONSTRAINT synchronization_ingest_batches_identity_unique
        UNIQUE (
            batch_id,
            reader_id
        ),

    CONSTRAINT synchronization_ingest_batches_batch_id_not_nil
        CHECK (
            batch_id
            <> '00000000-0000-0000-0000-000000000000'::UUID
        ),

    CONSTRAINT synchronization_ingest_batches_batch_id_version
        CHECK (
            SUBSTRING(batch_id::TEXT FROM 15 FOR 1) = '7'
        ),

    CONSTRAINT synchronization_ingest_batches_batch_id_variant
        CHECK (
            SUBSTRING(batch_id::TEXT FROM 20 FOR 1)
            IN ('8', '9', 'a', 'b')
        ),

    CONSTRAINT synchronization_ingest_batches_reader_id_not_nil
        CHECK (
            reader_id
            <> '00000000-0000-0000-0000-000000000000'::UUID
        ),

    CONSTRAINT synchronization_ingest_batches_reader_id_version
        CHECK (
            SUBSTRING(reader_id::TEXT FROM 15 FOR 1) = '7'
        ),

    CONSTRAINT synchronization_ingest_batches_reader_id_variant
        CHECK (
            SUBSTRING(reader_id::TEXT FROM 20 FOR 1)
            IN ('8', '9', 'a', 'b')
        ),

    CONSTRAINT synchronization_ingest_batches_protocol_version
        CHECK (
            protocol_version > 0
            AND protocol_version <= 65535
        ),

    CONSTRAINT synchronization_ingest_batches_environment
        CHECK (
            length(trim(environment_id)) > 0
            AND octet_length(environment_id) <= 128
        ),

    CONSTRAINT synchronization_ingest_batches_software_version
        CHECK (
            length(trim(reader_software_version)) > 0
            AND octet_length(reader_software_version) <= 128
        ),

    CONSTRAINT synchronization_ingest_batches_sequence_range
        CHECK (
            first_local_sequence_number > 0
            AND last_local_sequence_number
                >= first_local_sequence_number
        ),

    CONSTRAINT synchronization_ingest_batches_submission_time
        CHECK (
            submitted_at_unix_milliseconds >= 0
        ),

    CONSTRAINT synchronization_ingest_batches_receipt_time
        CHECK (
            received_at_unix_milliseconds >= 0
        ),

    CONSTRAINT synchronization_ingest_batches_entry_count
        CHECK (
            entry_count > 0
            AND entry_count <= 256
        ),

    CONSTRAINT synchronization_ingest_batches_request_fingerprint
        CHECK (
            request_fingerprint
            ~ '^[0-9a-f]{64}$'
        ),

    CONSTRAINT synchronization_ingest_batches_request_json
        CHECK (
            jsonb_typeof(canonical_request_json) = 'object'
        ),

    CONSTRAINT synchronization_ingest_batches_ack_fingerprint
        CHECK (
            acknowledgement_fingerprint
            ~ '^[0-9a-f]{64}$'
        ),

    CONSTRAINT synchronization_ingest_batches_ack_json
        CHECK (
            jsonb_typeof(
                canonical_acknowledgement_json
            ) = 'object'
        ),

    CONSTRAINT synchronization_ingest_batches_reader_fk
        FOREIGN KEY (reader_id)
        REFERENCES reader_equipment (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT
);

CREATE INDEX synchronization_ingest_batches_reader_sequence_index
    ON synchronization_ingest_batches (
        reader_id,
        first_local_sequence_number,
        last_local_sequence_number
    );

CREATE INDEX synchronization_ingest_batches_received_index
    ON synchronization_ingest_batches (
        received_at_unix_milliseconds
    );


CREATE TABLE synchronization_ingest_transactions (
    fare_transaction_id UUID PRIMARY KEY,

    reader_id UUID NOT NULL,

    local_sequence_number BIGINT NOT NULL,

    transaction_fingerprint TEXT NOT NULL,

    canonical_transaction_envelope_json JSONB NOT NULL,

    first_seen_batch_id UUID NOT NULL,

    current_resolution TEXT NOT NULL,

    first_received_at_unix_milliseconds BIGINT NOT NULL,

    last_resolved_at_unix_milliseconds BIGINT NOT NULL,

    created_at TIMESTAMPTZ NOT NULL
        DEFAULT CURRENT_TIMESTAMP,

    updated_at TIMESTAMPTZ NOT NULL
        DEFAULT CURRENT_TIMESTAMP,

    CONSTRAINT synchronization_ingest_transactions_identity
        UNIQUE (
            fare_transaction_id,
            reader_id,
            local_sequence_number
        ),

    CONSTRAINT synchronization_ingest_transactions_reader_sequence
        UNIQUE (
            reader_id,
            local_sequence_number
        ),

    CONSTRAINT synchronization_ingest_transactions_id_not_nil
        CHECK (
            fare_transaction_id
            <> '00000000-0000-0000-0000-000000000000'::UUID
        ),

    CONSTRAINT synchronization_ingest_transactions_id_version
        CHECK (
            SUBSTRING(
                fare_transaction_id::TEXT
                FROM 15 FOR 1
            ) = '7'
        ),

    CONSTRAINT synchronization_ingest_transactions_id_variant
        CHECK (
            SUBSTRING(
                fare_transaction_id::TEXT
                FROM 20 FOR 1
            ) IN ('8', '9', 'a', 'b')
        ),

    CONSTRAINT synchronization_ingest_transactions_reader_not_nil
        CHECK (
            reader_id
            <> '00000000-0000-0000-0000-000000000000'::UUID
        ),

    CONSTRAINT synchronization_ingest_transactions_sequence
        CHECK (
            local_sequence_number > 0
        ),

    CONSTRAINT synchronization_ingest_transactions_fingerprint
        CHECK (
            transaction_fingerprint
            ~ '^[0-9a-f]{64}$'
        ),

    CONSTRAINT synchronization_ingest_transactions_envelope
        CHECK (
            jsonb_typeof(
                canonical_transaction_envelope_json
            ) = 'object'
        ),

    CONSTRAINT synchronization_ingest_transactions_resolution
        CHECK (
            current_resolution IN (
                'acknowledged',
                'retryable_failure',
                'permanent_failure',
                'manual_review'
            )
        ),

    CONSTRAINT synchronization_ingest_transactions_times
        CHECK (
            first_received_at_unix_milliseconds >= 0
            AND last_resolved_at_unix_milliseconds
                >= first_received_at_unix_milliseconds
        ),

    CONSTRAINT synchronization_ingest_transactions_updated_time
        CHECK (
            updated_at >= created_at
        ),

    CONSTRAINT synchronization_ingest_transactions_first_batch_fk
        FOREIGN KEY (
            first_seen_batch_id,
            reader_id
        )
        REFERENCES synchronization_ingest_batches (
            batch_id,
            reader_id
        )
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    CONSTRAINT synchronization_ingest_transactions_reader_fk
        FOREIGN KEY (reader_id)
        REFERENCES reader_equipment (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT
);

CREATE INDEX synchronization_ingest_transactions_reader_resolution_index
    ON synchronization_ingest_transactions (
        reader_id,
        current_resolution,
        local_sequence_number
    );


CREATE TABLE synchronization_ingest_entries (
    batch_id UUID NOT NULL,

    reader_id UUID NOT NULL,

    entry_position INTEGER NOT NULL,

    fare_transaction_id UUID NOT NULL,

    local_sequence_number BIGINT NOT NULL,

    outcome TEXT NOT NULL,

    failure_category TEXT,

    next_retry_at_unix_milliseconds BIGINT,

    resolved_at_unix_milliseconds BIGINT NOT NULL,

    created_at TIMESTAMPTZ NOT NULL
        DEFAULT CURRENT_TIMESTAMP,

    PRIMARY KEY (
        batch_id,
        entry_position
    ),

    CONSTRAINT synchronization_ingest_entries_transaction_unique
        UNIQUE (
            batch_id,
            fare_transaction_id
        ),

    CONSTRAINT synchronization_ingest_entries_sequence_unique
        UNIQUE (
            batch_id,
            local_sequence_number
        ),

    CONSTRAINT synchronization_ingest_entries_position
        CHECK (
            entry_position >= 0
            AND entry_position < 256
        ),

    CONSTRAINT synchronization_ingest_entries_sequence
        CHECK (
            local_sequence_number > 0
        ),

    CONSTRAINT synchronization_ingest_entries_outcome
        CHECK (
            outcome IN (
                'acknowledged',
                'retryable_failure',
                'permanent_failure',
                'manual_review'
            )
        ),

    CONSTRAINT synchronization_ingest_entries_failure_category
        CHECK (
            failure_category IS NULL
            OR (
                length(trim(failure_category)) > 0
                AND octet_length(failure_category) <= 128
                AND failure_category
                    ~ '^[a-z][a-z0-9_]*$'
            )
        ),

    CONSTRAINT synchronization_ingest_entries_retry_time
        CHECK (
            next_retry_at_unix_milliseconds IS NULL
            OR next_retry_at_unix_milliseconds >= 0
        ),

    CONSTRAINT synchronization_ingest_entries_resolution_metadata
        CHECK (
            (
                outcome = 'acknowledged'
                AND failure_category IS NULL
                AND next_retry_at_unix_milliseconds IS NULL
            )
            OR (
                outcome = 'retryable_failure'
                AND failure_category IS NOT NULL
                AND next_retry_at_unix_milliseconds IS NOT NULL
            )
            OR (
                outcome IN (
                    'permanent_failure',
                    'manual_review'
                )
                AND failure_category IS NOT NULL
                AND next_retry_at_unix_milliseconds IS NULL
            )
        ),

    CONSTRAINT synchronization_ingest_entries_resolved_time
        CHECK (
            resolved_at_unix_milliseconds >= 0
        ),

    CONSTRAINT synchronization_ingest_entries_batch_fk
        FOREIGN KEY (
            batch_id,
            reader_id
        )
        REFERENCES synchronization_ingest_batches (
            batch_id,
            reader_id
        )
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    CONSTRAINT synchronization_ingest_entries_transaction_fk
        FOREIGN KEY (
            fare_transaction_id,
            reader_id,
            local_sequence_number
        )
        REFERENCES synchronization_ingest_transactions (
            fare_transaction_id,
            reader_id,
            local_sequence_number
        )
        ON UPDATE RESTRICT
        ON DELETE RESTRICT
);

CREATE INDEX synchronization_ingest_entries_transaction_history_index
    ON synchronization_ingest_entries (
        fare_transaction_id,
        resolved_at_unix_milliseconds
    );

CREATE INDEX synchronization_ingest_entries_batch_order_index
    ON synchronization_ingest_entries (
        batch_id,
        entry_position,
        local_sequence_number
    );

CREATE INDEX synchronization_ingest_entries_retry_index
    ON synchronization_ingest_entries (
        outcome,
        next_retry_at_unix_milliseconds
    )
    WHERE outcome = 'retryable_failure';
