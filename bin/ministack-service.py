#!/usr/bin/env python3
"""
MiniStack Local Cloud Emulator (Port 4566)
Provides lightweight emulation of AWS ECS, S3, RDS, and Health endpoints for LTN.ENG integration and testing.
Logs all requests to D:\tmp\ministack-4566.log.
"""

import http.server
import socketserver
import json
import os
import datetime

PORT = 4566
LOG_FILE = r"D:\tmp\ministack-4566.log"

os.makedirs(os.path.dirname(LOG_FILE), exist_ok=True)

class MiniStackHandler(http.server.BaseHTTPRequestHandler):
    def _log_request(self, status=200, action="GENERIC"):
        timestamp = datetime.datetime.now().isoformat()
        log_line = f"[{timestamp}] {self.command} {self.path} - Action: {action} - Status: {status}\n"
        try:
            with open(LOG_FILE, "a", encoding="utf-8") as f:
                f.write(log_line)
        except Exception:
            pass

    def do_GET(self):
        self._log_request(200, "HEALTH_CHECK")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        response = {
            "status": "healthy",
            "emulator": "MiniStack LocalCloud v1.0",
            "port": PORT,
            "services": ["ecs", "s3", "rds", "cloudwatch", "health"]
        }
        self.wfile.write(json.dumps(response).encode("utf-8"))

    def do_POST(self):
        content_length = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(content_length).decode("utf-8", errors="ignore") if content_length > 0 else ""

        # Determine target AWS action from headers or path
        target = self.headers.get("X-Amz-Target", "")
        action = target or self.path

        self._log_request(200, action)
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.end_headers()

        response = {
            "status": "SUCCESS",
            "message": f"MiniStack handled {action}",
            "details": {
                "service": "aws-emulator",
                "request_id": f"req-{datetime.datetime.now().strftime('%Y%m%d%H%M%S')}"
            }
        }
        self.wfile.write(json.dumps(response).encode("utf-8"))

    def do_PUT(self):
        self._log_request(200, "S3_PUT")
        self.send_response(200)
        self.send_header("Content-Type", "application/xml")
        self.end_headers()
        self.wfile.write(b"<CreateBucketResult><Status>Success</Status></CreateBucketResult>")

    def do_HEAD(self):
        self._log_request(200, "HEAD_CHECK")
        self.send_response(200)
        self.end_headers()

    def log_message(self, format, *args):
        # Suppress standard stderr logging to keep terminal clean
        return

def run_server():
    socketserver.TCPServer.allow_reuse_address = True
    with socketserver.TCPServer(("127.0.0.1", PORT), MiniStackHandler) as httpd:
        print(f"[*] MiniStack Local Cloud Emulator active on http://127.0.0.1:{PORT}")
        with open(LOG_FILE, "a", encoding="utf-8") as f:
            f.write(f"=== MiniStack Started on port {PORT} at {datetime.datetime.now().isoformat()} ===\n")
        try:
            httpd.serve_forever()
        except KeyboardInterrupt:
            pass

if __name__ == "__main__":
    run_server()
