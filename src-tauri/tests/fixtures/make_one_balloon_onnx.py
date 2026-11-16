"""Write one-balloon.onnx: a stand-in with the balloon model's contract
(input `images` [1, 3, 640, 640], output [1, 6, 2] YOLOv8 layout) that
always finds one dialogue box at centre (200, 200), 80 x 100 input pixels,
plus one box scoring too low to keep. Run: python make_one_balloon_onnx.py
"""

import os

import numpy as np
import onnx
from onnx import TensorProto, helper, numpy_helper

boxes = np.array(
    [[
        [200.0, 400.0],  # centre x
        [200.0, 400.0],  # centre y
        [80.0, 50.0],    # width
        [100.0, 50.0],   # height
        [0.9, 0.1],      # dialogue score
        [0.0, 0.2],      # effect score
    ]],
    dtype=np.float32,
)

nodes = [
    # The output depends on the input, so no runtime folds the graph away.
    helper.make_node("ReduceMean", ["images"], ["mean"], keepdims=1),
    helper.make_node("Mul", ["mean", "zero"], ["nothing"]),
    helper.make_node("Reshape", ["nothing", "shape"], ["flat"]),
    helper.make_node("Add", ["flat", "boxes"], ["output0"]),
]
graph = helper.make_graph(
    nodes,
    "one_balloon",
    [helper.make_tensor_value_info("images", TensorProto.FLOAT, [1, 3, 640, 640])],
    [helper.make_tensor_value_info("output0", TensorProto.FLOAT, [1, 6, 2])],
    [
        numpy_helper.from_array(np.zeros((1,), np.float32), "zero"),
        numpy_helper.from_array(np.array([1, 1, 1], np.int64), "shape"),
        numpy_helper.from_array(boxes, "boxes"),
    ],
)
model = helper.make_model(graph, opset_imports=[helper.make_opsetid("", 13)])
model.ir_version = 8
onnx.checker.check_model(model)
onnx.save(model, os.path.join(os.path.dirname(os.path.abspath(__file__)), "one-balloon.onnx"))
