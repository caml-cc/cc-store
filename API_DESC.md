**File Service API**

This service stores uploaded files on disk and records their original filenames in SQLite. The server listens on the port from `PORT` in the environment, and requires `API_KEY` for write operations, as loaded in pkg/utils/config.go.

**Authentication**

Write endpoints require the request header `K` to match the configured API key.

- Required for: upload and delete
- Not required for: download

**Base URL**

- `http://{host}:{PORT}`

**Endpoints**

1. **POST /**

   Upload a single file.

   - Auth: required
   - Content type: multipart/form-data
   - Required form field: `file`
   - Success status: 201 Created
   - Success response: plain text containing `{host}/{generated-id}` plus a trailing newline
   - Response content type: application/text

   Behavior:
   - Generates a random 10-character alphanumeric id
   - Stores the file on disk at `internal/uploads/{id}`
   - Saves metadata in SQLite with:
     - id
     - original filename
     - expiration timestamp set to 24 hours from upload
   - The original filename is preserved for download responses

   Common errors:
   - 401 Unauthorized if `K` is missing or invalid
   - 400 Bad Request if the multipart form or `file` field is missing
   - 500 Internal Server Error if storage or database write fails

2. **GET /{id}**

   Download a file by its id.

   - Auth: not required
   - Path parameter:
     - `id`: the generated file id
   - Success status: 200 OK
   - Success response: raw file bytes
   - Response content type: application/octet-stream
   - Response header: Content-Disposition: attachment; filename={original filename}

   Behavior:
   - Looks up the original filename from SQLite
   - Opens the file from `internal/uploads/{id}`
   - Streams the file to the client as an attachment

   Common errors:
   - 400 Bad Request if `id` is empty
   - 404 Not Found if the file is missing on disk

   Implementation note:
   - If the database record is missing but the file still exists, the server still serves the file, but the download filename may be empty because the current handler does not stop on the database lookup error.

3. **DELETE /{id}**

   Delete a stored file.

   - Auth: required
   - Path parameter:
     - `id`: the generated file id
   - Success status: 204 No Content

   Behavior:
   - Removes `internal/uploads/{id}` from disk

   Common errors:
   - 401 Unauthorized if `K` is missing or invalid
   - 400 Bad Request if `id` is empty
   - 404 Not Found if the file does not exist
   - 500 Internal Server Error if deletion fails for another reason

   Implementation note:
   - The current handler removes only the file from disk. It does not delete the corresponding SQLite row, so metadata can become stale.

**Observed data model**

From internal/database/migrations/20260415124521-file.sql, the table stores:

- `id`
- `filename`
- `created_at`
- `expires_at`

The API currently does not enforce expiration during upload, download, or delete.