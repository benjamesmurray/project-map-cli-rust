package com.example

import com.example.util.NetworkUtils
import com.example.util.helper

fun main() {
    val utils = NetworkUtils()
    println(utils.fetchData())
    helper()
}
