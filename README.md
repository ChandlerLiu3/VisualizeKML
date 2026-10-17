# VisualizeKML
在三维空间中观看轨迹，掌握路线的高低俯仰之势。

## 运行

需要安装 Rust stable 工具链和可用的桌面图形环境。

```powershell
cargo run --release
```

启动时会弹出 KML 文件选择框；也可以直接传入文件路径：

```powershell
cargo run --release -- "D:\path\to\track.kml"
```

窗口中按 `Ctrl+O` 可加载其他 KML 文件。鼠标左键拖动可旋转视角，滚轮可缩放。
支持 `LineString`/`MultiGeometry` 的 `coordinates` 和 Google 扩展的 `gx:Track/gx:coord`；
尚不支持在线地图、样式解析、右键平移或轨迹动画。

## 开发与测试

```powershell
cargo test
cargo run
```
