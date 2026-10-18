# VisualizeKML
在三维空间中观看轨迹，掌握路线的高低俯仰之势。

当前版本：**0.3.0**。支持提取 KML `Point` 标记，推断缺失的起点/终点，并标出轨迹全局最高点和最低点。
纬度增大映射为北向（+Z），经度增大映射为东向（+X）；初始视角中北向显示在上方、东向显示在右侧。
南北方向统一依据带符号纬度差计算：北纬 30° 到 31°向北，南纬 30°S 到 31°S 向南；海拔仍独立映射到 +Y。

## 运行

需要安装 Rust stable 工具链和可用的桌面图形环境。

```powershell
cargo run --release
```

启动时会弹出 KML 文件选择框；也可以直接传入文件路径：

```powershell
cargo run --release -- "D:\path\to\track.kml"
```

查看程序版本：

```powershell
visualize_kml.exe --version
```

轨迹按全部有效海拔范围从冷色渐变至暖色，并在界面图例中显示对应海拔范围（相对高度视觉编码，不代表温度）。
缺少海拔的点使用中性色；轨迹无有效海拔或海拔全部相同时，整条轨迹使用中性色。多个轨迹段共享同一海拔范围。
标记旁会显示有效高程的取整数值；贴地模式不显示数值。
方向箭头按每段累计路线距离每 500 m 布置，并根据当前屏幕间距隐藏重叠箭头；轨迹短于间隔时不强制添加箭头。

Windows Release 发布文件使用版本化名称 `dist/visualize_kml-0.3.0-windows-x86_64.exe`，
EXE 文件属性包含产品和文件版本。构建发布版本：

```powershell
cargo build --release
New-Item -ItemType Directory -Force dist | Out-Null
Copy-Item target\release\visualize_kml.exe dist\visualize_kml-0.3.0-windows-x86_64.exe
```

窗口中按 `Ctrl+O` 可加载其他 KML 文件。鼠标左键拖动可旋转视角，滚轮可缩放。
起点使用沿路线方向的三角形，终点使用棋盘旗；只有标记来源提供了非空名称时才显示名称。
无名起终点仍显示专属图标，其他无名标记显示为点。标记列表空间有限时只列出前 14 个，
其余仍显示在场景中。若轨迹不含有效海拔，会显示提示且不生成高程标记。
由于当前没有至少 5 条人工标注真实轨迹完成验证，局部山峰候选暂不显示；验证启用门槛为精确率不低于 90%、召回率不低于 80%。
描述、样式引用和扩展数据会被解析保留，
但不会加载描述中的远程图片或链接。中文标签使用系统已安装的中文字体。

支持 `LineString`/`MultiGeometry` 的 `coordinates` 和 Google 扩展的 `gx:Track/gx:coord`。
在线地图、H 键快捷操作、右键平移、轨迹动画和 KML 样式渲染仍属于后续版本范围。

## 开发与测试

Windows 构建会用 Windows SDK Resource Compiler 写入 EXE 文件版本信息。

```powershell
cargo test
cargo run
```
