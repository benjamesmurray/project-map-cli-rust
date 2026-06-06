class_name Player
extends "res://tests/fixtures/gdscript/item.gd"

signal health_changed(old_value, new_value)

var weapon = preload("res://tests/fixtures/gdscript/weapon.gd")

func take_damage(amount):
    print("Ouch!")
    health_changed.emit(100, 100 - amount)
