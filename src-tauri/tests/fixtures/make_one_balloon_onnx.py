"""Write one-balloon.onnx: a stand-in with the balloon model's contract
(inputs `images` [1, 3, 640, 640] and `orig_target_sizes` [1, 2] as width,
height; outputs `labels` [1, 3], `boxes` [1, 3, 4] x1 y1 x2 y2 in screen
pixels, `scores` [1, 3]). It finds one dialogue text area at 0.2 .. 0.4 of
the width by 0.2 .. 0.3 of the height, a balloon outline (label 0) and a
text area scoring too low to keep. Run: python -I make_one_balloon_onnx.py
"""

import os

import numpy as np
import onnx
from onnx import TensorProto, helper, numpy_helper

fractions = np.array(
    [[[0.2, 0.2, 0.4, 0.3], [0.1, 0.1, 0.5, 0.4], [0.6, 0.6, 0.7, 0.7]]],
    dtype=np.float32,
)

nodes = [
    # Boxes scale with the screen size, as the real model's do.
    helper.make_node("Cast", ["orig_target_sizes"], ["size"], to=TensorProto.FLOAT),
    helper.make_node("Concat", ["size", "size"], ["wh_wh"], axis=1),
    helper.make_node("Unsqueeze", ["wh_wh", "axis1"], ["scale"]),
    helper.make_node("Mul", ["fractions", "scale"], ["boxes"]),
    # Scores depend on the picture, so no runtime folds the graph away.
    helper.make_node("ReduceMean", ["images"], ["mean"], keepdims=0),
    helper.make_node("Mul", ["mean", "zero"], ["nothing"]),
    helper.make_node("Add", ["nothing", "base_scores"], ["scores"]),
    helper.make_node("Identity", ["base_labels"], ["labels"]),
]
graph = helper.make_graph(
    nodes,
    "one_balloon",
    [
        helper.make_tensor_value_info("images", TensorProto.FLOAT, [1, 3, 640, 640]),
        helper.make_tensor_value_info("orig_target_sizes", TensorProto.INT64, [1, 2]),
    ],
    [
        helper.make_tensor_value_info("labels", TensorProto.INT64, [1, 3]),
        helper.make_tensor_value_info("boxes", TensorProto.FLOAT, [1, 3, 4]),
        helper.make_tensor_value_info("scores", TensorProto.FLOAT, [1, 3]),
    ],
    [
        numpy_helper.from_array(fractions, "fractions"),
        numpy_helper.from_array(np.array([1], np.int64), "axis1"),
        numpy_helper.from_array(np.zeros((1,), np.float32), "zero"),
        numpy_helper.from_array(np.array([[0.9, 0.95, 0.1]], np.float32), "base_scores"),
        numpy_helper.from_array(np.array([[1, 0, 1]], np.int64), "base_labels"),
    ],
)
model = helper.make_model(graph, opset_imports=[helper.make_opsetid("", 13)])
model.ir_version = 8
onnx.checker.check_model(model)
onnx.save(model, os.path.join(os.path.dirname(os.path.abspath(__file__)), "one-balloon.onnx"))
