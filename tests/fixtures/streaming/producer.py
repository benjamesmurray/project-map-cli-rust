import os

def publish():
    producer.send("wde.labels.phase.v1", value=b"sample")
    port = os.getenv("PORT")
