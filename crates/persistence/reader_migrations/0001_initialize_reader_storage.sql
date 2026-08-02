CREATE TABLE reader_state (
    singleton INTEGER NOT NULL
        PRIMARY KEY
        CHECK (singleton = 1),

    reader_id TEXT NOT NULL
        UNIQUE
        CHECK (length(reader_id) = 36),

    environment_id TEXT NOT NULL
        CHECK (length(trim(environment_id)) > 0),

    software_version TEXT NOT NULL
        CHECK (length(trim(software_version)) > 0),

    protocol_version INTEGER NOT NULL
        CHECK (
            protocol_version > 0
            AND protocol_version <= 65535
        ),

    next_local_sequence INTEGER NOT NULL
        DEFAULT 1
        CHECK (next_local_sequence > 0),

    last_acknowledged_sequence INTEGER NOT NULL
        DEFAULT 0
        CHECK (
            last_acknowledged_sequence >= 0
            AND last_acknowledged_sequence
                < next_local_sequence
        ),

    created_at_unix_milliseconds INTEGER NOT NULL
        CHECK (
            created_at_unix_milliseconds >= 0
        ),

    updated_at_unix_milliseconds INTEGER NOT NULL
        CHECK (
            updated_at_unix_milliseconds
                >= created_at_unix_milliseconds
        )
);
