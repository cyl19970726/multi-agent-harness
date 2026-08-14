# Measurement Toolbox · 测量与渲染验证工具箱

Phase 2（测量成规格）与 Phase 3（渲染验证）的可执行工具集。
原则：**所有美术结论必须可由脚本复测**；凭肉眼缩略图下结论 = 失效模式。

## 1. 像素采样（PIL）——色板与"生效与否"判定

```python
from PIL import Image
from collections import Counter
im = Image.open('shot.png').convert('RGB')

# 区域主色（色板实测：canvas / 卡片 / 发丝线 / 墨色）
def region_hist(x0,y0,x1,y1,n=4):
    px=[im.getpixel((x,y)) for x in range(x0,x1,2) for y in range(y0,y1,2)]
    return ['#%02x%02x%02x x%d'%(p[0],p[1],p[2],c) for p,c in Counter(px).most_common(n)]

# 边线存在性：扫一条线，找显著暗于两侧背景的像素
def edge_scan_horizontal(y, x0, x1, threshold=680):
    return [x for x in range(x0,x1) if sum(im.getpixel((x,y))) < threshold]
```

两条实测纪律：

- **对比度 < 人眼阈 = 不存在**。白卡 `#ffffff` on 画布 `#fffefb`（差 1/255）
  和"没画卡片"视觉等价；材质类改动必须采样证明差异可测，不是规则写了就算
- **规则存在 ≠ 已渲染**。层叠故障（`@layer` 被 preflight 静默覆盖、优先级反转）
  会让整张样式表部分失效且无任何报错——见 §4

## 2. 几何测量——字阶与间距

```python
# 文本行高：在已知文本区域找暗像素行聚簇，行数与行高即字阶依据
# 卡片 padding / 间距：沿边线法向采样，边线到首个文字像素的距离
# 栏宽：整列扫描垂直发丝线位置（连续暗像素列）
```

几何结论按**设计图基准宽度等比换算**到实现视口（如 1504 基准 → 1440 实现）。

## 3. 并排 montage——Owner 验收的标准输入

```python
from PIL import Image
d = Image.open('design.png').convert('RGB')
a = Image.open('actual.png').convert('RGB')
d2 = d.resize((a.size[0], round(d.size[1]*a.size[0]/d.size[0])))  # 同宽对齐
H = max(d2.size[1], a.size[1])
m = Image.new('RGB', (a.size[0]*2+8, H), (255,0,0))
m.paste(d2,(0,0)); m.paste(a,(a.size[0]+8,0)); m.save('side-by-side.png')
```

验收裁切用**原生分辨率**（ReadMediaFile 的 region 参数）；缩略图会吃掉
1px 发丝线和 1/255 填充差，造成"没生效"的误判（本工具箱存在的主要原因）。

## 4. 层叠与产物探针——"改了没渲染"的根因排查

当像素证据显示规则未生效，按序排查（每一步都是实测，不猜）：

1. **源文件**：规则真的在文件里、无语法断裂（缺括号会让后续规则静默丢弃）
2. **服务产物**：dev server 实际吐出的 CSS 是否含该规则
   （vite：必须从仓根 `pnpm exec vite --config <path>/vite.config.ts` 起，
   config 的 `root` 相对 cwd 解析，起错位置全 404——实测坑）
3. **层叠**：`@layer` 顺序、unlayered vs layered 优先级、preflight 重置
   （实测：`@layer components` 使 Tailwind preflight 的 `margin:0;padding:0;
   border:0` 静默 nullify 整张表的对应声明，数月未被发现）
4. **computed style / DOM 几何**：playwright `getComputedStyle` +
   `getBoundingClientRect` 定量（例：按钮宽 75px≠35px 暴露了 min-width
   通用规则压过方形声明）
5. **像素复测**：修完回到 §1 证明生效

## 5. 证据绑定与目录约定

- 截图文件名/manifest 带 **exact 40 位 SHA**；"通过"必须能回答"哪个 revision、
  哪个验证表面"
- 证据目录 gitignored（如 `.visual-evidence/`）；fixture 证据与 real-data 证据分开标注
- 捕获即测试：截图由项目 browser-check 产出，断言与证据同一次运行生成，
  不许手工补截"好看的一次"

## 6. 差异定位（round 间对比）

```python
from PIL import ImageChops
diff = ImageChops.difference(prev.crop(box), curr.crop(box))
# diff.getbbox() + 列直方图：结构性变化（边线）vs 文本变化的区分
# 边线新增 = 单列高计数；文本变化 = 散布在文本区
```

用于区分"我这轮改动到底渲染出了什么"，防止把文本差异误读为结构差异。
