CREATE UNIQUE INDEX
    offline_transactions_identity_sequence_idx
ON offline_transactions (
    fare_transaction_id,
    reader_id,
    local_sequence_number
);

CREATE TABLE synchronization_batches (
    batch_id TEXT NOT NULL
        PRIMARY KEY
        CHECK (length(batch_id) = 36),

    reader_id TEXT NOT NULL
        CHECK (length(reader_id) = 36),

    protocol_version INTEGER NOT NULL
        CHECK (
            protocol_version > 0
            AND protocol_version <= 65535
        ),

    first_local_sequence_number INTEGER NOT NULL
        CHECK (first_local_sequence_number > 0),

    last_local_sequence_number INTEGER NOT NULL
        CHECK (
            last_local_sequence_number
                >= first_local_sequence_number
        ),

    batch_state TEXT NOT NULL
        DEFAULT 'prepared'
        CHECK (
            batch_state IN (
                'prepared',
                'in_flight',
                'retryable_failure',
                'acknowledged',
                'permanent_failure',
                'manual_review'
            )
        ),

    attempt_count INTEGER NOT NULL
        DEFAULT 0
        CHECK (attempt_count >= 0),

    next_retry_at_unix_milliseconds INTEGER
        CHECK (
            next_retry_at_unix_milliseconds IS NULL
            OR next_retry_at_unix_milliseconds >= 0
        ),

    last_failure_category TEXT,

    created_at_unix_milliseconds INTEGER NOT NULL
        CHECK (created_at_unix_milliseconds >= 0),

    updated_at_unix_milliseconds INTEGER NOT NULL
        CHECK (
            updated_at_unix_milliseconds
                >= created_at_unix_milliseconds
        ),

    UNIQUE (
        batch_id,
        reader_id
    ),

    FOREIGN KEY (reader_id)
        REFERENCES reader_state (reader_id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT
);

CREATE TABLE synchronization_entries (
    batch_id TEXT NOT NULL
        CHECK (length(batch_id) = 36),

    reader_id TEXT NOT NULL
        CHECK (length(reader_id) = 36),

    fare_transaction_id TEXT NOT NULL
        CHECK (length(fare_transaction_id) = 36),

    local_sequence_number INTEGER NOT NULL
        CHECK (local_sequence_number > 0),

    entry_position INTEGER NOT NULL
        CHECK (entry_position >= 0),

    PRIMARY KEY (
        batch_id,
        fare_transaction_id
    ),

    UNIQUE (fare_transaction_id),

    UNIQUE (
        batch_id,
        local_sequence_number
    ),

    UNIQUE (
        batch_id,
        entry_position
    ),

    FOREIGN KEY (
        batch_id,
        reader_id
    )
        REFERENCES synchronization_batches (
            batch_id,
            reader_id
        )
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    FOREIGN KEY (
        fare_transaction_id,
        reader_id,
        local_sequence_number
    )
        REFERENCES offline_transactions (
            fare_transaction_id,
            reader_id,
            local_sequence_number
        )
        ON UPDATE RESTRICT
        ON DELETE RESTRICT
);

CREATE INDEX synchronization_batches_ready_idx
    ON synchronization_batches (
        reader_id,
        batch_state,
        next_retry_at_unix_milliseconds,
        first_local_sequence_number
    );
