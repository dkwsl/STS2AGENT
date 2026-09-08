"""Windows-side wrapper: runs the STS2 MCP server from WSL filesystem."""
import os
import sys

# WSL UNC path to the mcp directory
mcp_dir = r"\\wsl.localhost\Ubuntu-24.04\home\aaa12321\sts2mcp\STS2MCP\mcp"

# Read server.py with UTF-8 (Windows default is GBK, would crash)
server_path = os.path.join(mcp_dir, "server.py")
with open(server_path, encoding="utf-8") as f:
    code = f.read()

# Execute as if __main__
exec(compile(code, server_path, "exec"), {"__name__": "__main__"})
