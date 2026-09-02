package com.wde.streaming

class Processor {
    fun process() {
        val topic = "wde.labels.phase.v1"
        builder.stream("wde.bars.raw.v5").to(topic)
        val dbPass = System.getenv("DB_PASSWORD")
    }
}
