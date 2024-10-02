erDiagram
    USERS ||--o{ CONSENTS : has
    USERS {
        uuid id PK
        varchar username
        varchar email
        varchar password_hash
        boolean mfa_enabled
        varchar mfa_secret
        jsonb additional_info
    }
    CONSENTS {
        uuid id PK
        uuid user_id FK
        varchar scope
        timestamp created_at
        timestamp expires_at
        varchar status
    }
    AUDIT_LOGS {
        uuid id PK
        timestamp event_time
        varchar event_type
        uuid user_id
        jsonb event_details
        varchar previous_hash
        varchar current_hash
    }
