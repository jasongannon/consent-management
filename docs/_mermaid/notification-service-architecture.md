graph TD
    A[Event Source] -->|Events| B[Message Queue]
    B --> C[Notification Service]
    C --> D[Email Service]
    C --> E[SMS Service]
    C --> F[Push Notification Service]
    C <--> G[(Database)]
    H[Retry Manager] --> C
    I[User Preference Manager] --> C
    J[Delivery Status Tracker] --> C
