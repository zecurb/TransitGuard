CREATE TABLE synchronization_acknowledgements (
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

    received_at_unix_milliseconds INTEGER NOT NULL
        CHECK (received_at_unix_milliseconds >= 0),

    payload_json TEXT NOT NULL
        CHECK (length(trim(payload_json)) > 0),

    applied_at_unix_milliseconds INTEGER
        CHECK (
            applied_at_unix_milliseconds IS NULL
            OR applied_at_unix_milliseconds
                >= received_at_unix_milliseconds
        ),

    UNIQUE (
        batch_id,
        reader_id
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
        ON DELETE RESTRICT
);

CREATE TABLE synchronization_acknowledgement_entries (
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

    outcome TEXT NOT NULL
        CHECK (
            outcome IN (
                'acknowledged',
                'retryable_failure',
                'permanent_failure',
                'manual_review'
            )
        ),

    failure_category TEXT,

    retry_at_unix_milliseconds INTEGER
        CHECK (
            retry_at_unix_milliseconds IS NULL
            OR retry_at_unix_milliseconds >= 0
        ),

    PRIMARY KEY (
        batch_id,
        fare_transaction_id
    ),

    UNIQUE (
        batch_id,
        local_sequence_number
    ),

    UNIQUE (
        batch_id,
        entry_position
    ),

    CHECK (
        (
            outcome = 'acknowledged'
            AND failure_category IS NULL
            AND retry_at_unix_milliseconds IS NULL
        )
        OR (
            outcome = 'retryable_failure'
            AND length(trim(failure_category)) > 0
            AND retry_at_unix_milliseconds IS NOT NULL
        )
        OR (
            outcome IN (
                'permanent_failure',
                'manual_review'
            )
            AND length(trim(failure_category)) > 0
            AND retry_at_unix_milliseconds IS NULL
        )
    ),

    FOREIGN KEY (
        batch_id,
        reader_id
    )
        REFERENCES synchronization_acknowledgements (
            batch_id,
            reader_id
        )
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    FOREIGN KEY (
        batch_id,
        fare_transaction_id
    )
        REFERENCES synchronization_entries (
            batch_id,
            fare_transaction_id
        )
        ON UPDATE RESTRICT
        ON DELETE RESTRICT
);

CREATE INDEX synchronization_acknowledgements_unapplied_idx
    ON synchronization_acknowledgements (
        reader_id,
        applied_at_unix_milliseconds,
        received_at_unix_milliseconds
    );
