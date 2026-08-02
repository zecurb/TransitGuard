CREATE TABLE offline_transactions (
    fare_transaction_id TEXT NOT NULL
        PRIMARY KEY
        CHECK (length(fare_transaction_id) = 36),

    reader_id TEXT NOT NULL
        CHECK (length(reader_id) = 36),

    local_sequence_number INTEGER NOT NULL
        CHECK (local_sequence_number > 0),

    fare_credential_id TEXT NOT NULL
        CHECK (length(fare_credential_id) = 36),

    event_time_unix_milliseconds INTEGER NOT NULL
        CHECK (event_time_unix_milliseconds >= 0),

    fare_policy_version INTEGER NOT NULL
        CHECK (fare_policy_version > 0),

    processing_mode TEXT NOT NULL
        DEFAULT 'offline'
        CHECK (processing_mode = 'offline'),

    provisional_decision_json TEXT NOT NULL
        CHECK (
            length(trim(provisional_decision_json)) > 0
        ),

    transaction_envelope_json TEXT NOT NULL
        CHECK (
            length(trim(transaction_envelope_json)) > 0
        ),

    queue_state TEXT NOT NULL
        DEFAULT 'pending'
        CHECK (
            queue_state IN (
                'pending',
                'in_flight',
                'acknowledged',
                'retryable_failure',
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
        reader_id,
        local_sequence_number
    ),

    FOREIGN KEY (reader_id)
        REFERENCES reader_state (reader_id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT
);

CREATE INDEX offline_transactions_queue_order_idx
    ON offline_transactions (
        reader_id,
        queue_state,
        local_sequence_number
    );
