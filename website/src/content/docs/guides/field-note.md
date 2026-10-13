---
title: 一次桌面文件测试
description: 2026-10-13，同一句要求交给三条模型渠道，执行路径并不一样。
---

2026 年 10 月 13 日，用同一句话试了三次：在桌面放一份 Word 和一份 Excel，内容随便写。

本地工具的入口是一样的。exec 收的是一段 JavaScript，真正写文件要在里面再调用命令。Word 的技能文档要求用 Python，表格的技能文档要求用 Node。网关没有替模型选语言。

硅基流动上的 DeepSeek-V4-Flash 走 Chat。它先把 require 和 shell 直接填进 JavaScript，后来又把一段 Python 源码填进去，语法坏了几次。绕开之后，Word 用 python-docx 写成，表格按技能用了 Node。看渲染图时，这家接口回答这个模型不是视觉模型，那一轮就停了。文件最后还是拷到了桌面。

onomeo 上的 DeepSeek-V4.1-Flash 走 Responses。它也把 exec 的写法试错过一次，随后改用本机 Python，两份文件都做完了。回复末尾有中继自己加上的 via onomeo.com。

同一套隔离环境里的 Grok 走 Responses。外层 JavaScript 一次写对，里面再调用 Python 和 Node，没有在语法上打转。

协议只决定工具长什么样。会不会绕路，是模型自己的差别。渠道也会改能力：DeepSeek 官方的 Flash 可以看图，硅基流动上的 V4-Flash 这一次不行。这则记录只说明当天看到的事，不是能力承诺。
