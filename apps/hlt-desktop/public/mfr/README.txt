encoder_model.onnx · decoder_model.onnx · tokenizer.json — Pix2Text-MFR (MIT, https://huggingface.co/breezedeus/pix2text-mfr) 의 8비트 양자화본.
수식 그림 읽기(src/formula-ocr.ts)가 처음 쓸 때 싣는다. scripts/vendor.mjs 가 릴리스 자산(mfr-model-1)에서 받고 sha256 을 확인한다.

만드는 법(원본 ONNX → 양자화 · tokenizer.json 은 그대로):
  from onnxruntime.quantization import quantize_dynamic, QuantType
  quantize_dynamic('encoder_model.onnx', 'out/encoder_model.onnx', weight_type=QuantType.QUInt8)
  quantize_dynamic('decoder_model.onnx', 'out/decoder_model.onnx', weight_type=QuantType.QUInt8)
