#!/usr/bin/env bash
# fakeserver.sh — a stand-in for a real Minecraft server, used to exercise the
# mcsd supervisor without downloading a jar or booting a JVM.
#
# Behaviour that mimics a Minecraft server:
#   - prints a startup banner and a "Done" line
#   - reads console commands from stdin
#   - on "stop", saves and exits cleanly (0)
#   - on "crash", exits non-zero to test restart/backoff
#
# Point a server config at it with:
#   "java_path": "/absolute/path/to/examples/fakeserver.sh",
#   "jar": "",            # empty: no -jar argument is added
#   "server_args": []

echo "Starting fake Minecraft server..."
sleep 0.3
echo 'Done (0.3s)! For help, type "help"'

while IFS= read -r line; do
  echo "> ${line}"
  case "${line}" in
    stop)
      echo "Stopping server"
      echo "Saving worlds..."
      sleep 0.3
      echo "Saved. Goodbye."
      exit 0
      ;;
    crash)
      echo "Simulating crash!"
      exit 1
      ;;
  esac
done

# stdin closed without a stop command.
echo "Console input closed; exiting."
