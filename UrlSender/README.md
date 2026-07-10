# Url Sender

一个基于 Chrome Manifest V3 的浏览器插件。

功能说明：

- 点击插件图标后，可以配置后台地址。
- 可以分别配置“中文输入法网址规则”和“英文输入法网址规则”。
- 新增规则时可以在弹窗中选择“关键词 / 域名 / 前缀 / 正则”后添加。
- 每行一条规则，支持关键词匹配、域名精准匹配、域名前缀匹配、正则匹配。
- 当访问新的网页地址时：
  - 命中中文规则，调用 `baseUrl/switch?mode=1`
  - 命中英文规则，调用 `baseUrl/switch?mode=0`
- 中文规则优先于英文规则。
- 命中后插件图标会短暂显示 `CN`、`EN` 或 `ERR`。

## 加载方式

1. 打开 Chrome。
2. 进入 `chrome://extensions/`
3. 打开右上角“开发者模式”。
4. 点击“加载已解压的扩展程序”。
5. 选择当前项目目录 `UrlSender`。

## 配置示例

后台地址：

```text
http://127.0.0.1:5998
```

中文输入法网址规则：

```text
关键词: docs
域名: baidu.com
前缀: https://baidu.com/query
正则: ^https://mail\.qq\.com/
```

英文输入法网址规则：

```text
关键词: github
域名: github.com
前缀: https://stackoverflow.com/questions
正则: ^https://.*\.example\.com/
```

## 规则写法

- `关键词: github`：只要 URL 中包含 `github` 就命中，不区分大小写。
- `域名: baidu.com`：只匹配域名完全等于 `baidu.com` 的网页。`www.baidu.com`、`tieba.baidu.com` 不会命中。
- `前缀: https://baidu.com/query`：匹配相同协议、域名和路径前缀。比如 `https://baidu.com/query?123`、`https://baidu.com/query/1/2/3` 都会命中。
- `正则: ^https://mail\.qq\.com/`：按 JavaScript 正则匹配整条 URL。

为了兼容旧配置，不写前缀的行仍然按正则处理。
