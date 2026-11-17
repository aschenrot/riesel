import json
import socket

def request(operation, **arguments):
    message = {
        "version": 1,
        "request": {
            "operation": operation,
            **arguments,
        },
    }

    with socket.create_connection(("127.0.0.1", 47911), timeout=5) as connection:
        connection.settimeout(5)

        with connection.makefile("rwb") as stream:
            stream.write((json.dumps(message) + "\n").encode())
            stream.flush()

            return json.loads(stream.readline())

created = request(
    "create",
    model="task",
    data={"title": "Test Riesel", "completed": False},
)

print("Created:", created)

listed = request("list", model="task")

print("Listed:", listed)

assert created["Success"]["Record"]["revision"] == 1
assert len(listed["Success"]["Records"]) >= 1

print("Storage process test passed")