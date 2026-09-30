// Every string the page shows, in English and Chinese. Pure data, so the
// smoke test can check that both languages cover the same keys and every
// reader and label choice the converter offers.

export const LANGUAGES = ['en', 'zh'];

export const TEXT = {
  en: {
    ui: {
      toggle: '中文',
      toggleLabel: 'Switch the page to Chinese',
      tagline: 'Convert a MOBI dictionary to StarDict, right in your browser.',
      privacy: 'Your file never leaves this device: the converter runs inside this browser tab, and nothing is uploaded.',
      choose: 'Choose a MOBI dictionary, or drop it here',
      chosen: 'Selected: {name} ({size})',
      reader: 'Reader',
      labels: 'Language of added labels',
      labelsHint: 'Used for the lookup keys mobi2star adds for chapters and image galleries.',
      convert: 'Convert',
      cancel: 'Cancel',
      cancelled: 'Conversion cancelled.',
      mobile: 'Large dictionaries need a lot of memory; on phones and tablets the conversion may fail.',
      done: 'Done',
      checksPassed: 'All checks that run during conversion passed.',
      dictionary: 'Dictionary',
      headwords: 'Headwords',
      aliases: 'Inflections and aliases',
      entries: 'Index entries',
      source: 'Converted from',
      sourceSrcs: 'the embedded publisher source',
      sourceCompiled: 'the compiled MOBI text',
      download: 'Download {name} ({size})',
      install: 'Install',
      details: 'What was checked (in English)',
      notes: 'Notes on these checks (in English)',
      errorDetail: 'Details: {detail}',
      cliHint: 'The command-line version has higher limits and more checks.',
      version: 'mobi2star {version}',
      sourceCode: 'Source code',
      readerNotes: 'Reader notes',
      license: 'Convert only dictionaries you are entitled to use.',
    },
    readers: {
      koreader: {
        name: 'KOReader',
        description: 'KOReader on e-readers and Android. Also works for GoldenDict on desktop. Smallest output.',
        install: [
          'Unzip the download.',
          'Copy the folder into koreader/data/dict/ inside KOReader’s folder on your device.',
          'Restart KOReader.',
        ],
      },
      goldendict: {
        name: 'GoldenDict (desktop)',
        description: 'GoldenDict and GoldenDict-ng on desktop, and SilverDict. Same output as KOReader.',
        install: [
          'Unzip the download.',
          'In GoldenDict, open Edit → Dictionaries → Sources → Files.',
          'Add the folder, then choose Rescan.',
        ],
      },
      'goldendict-mobile': {
        name: 'GoldenDict Mobile (Android)',
        description: 'GoldenDict Mobile on Android. Every entry carries its own copy of the stylesheet.',
        install: [
          'Unzip the download.',
          'Copy the folder into the GoldenDict folder on your device’s storage.',
          'Rescan dictionaries in GoldenDict.',
        ],
      },
      readest: {
        name: 'Readest',
        description: 'Readest. Every entry carries its own copy of the stylesheet.',
        install: [
          'Unzip the download.',
          'Import the files in the folder from Readest’s dictionary settings.',
        ],
      },
      kobo: {
        name: 'Kobo (via PyGlossary or penelope)',
        description: 'For building a Kobo dictionary with PyGlossary or penelope. Every entry carries its own copy of the stylesheet.',
        install: [
          'Unzip the download.',
          'Convert the folder with PyGlossary or penelope into a Kobo dictionary.',
          'Copy the result into .kobo/custom-dict/ on your Kobo.',
        ],
      },
      universal: {
        name: 'Several or other readers',
        description: 'For several readers at once, or untested ones such as Boox. Largest output.',
        install: [
          'Unzip the download.',
          'Add the folder to each reader the way it expects; see the notes for KOReader and GoldenDict.',
        ],
      },
    },
    labels: {
      en: 'English',
      zh: 'Chinese',
    },
    stages: {
      starting: 'Loading the converter…',
      parsing: 'Reading the dictionary…',
      rendering: 'Converting entries: {done} of {total}',
      writing: 'Writing the index…',
      checking: 'Checking the output…',
    },
    errors: {
      MALFORMED: 'This file is not a MOBI dictionary that mobi2star can read.',
      UNSUPPORTED: 'This dictionary uses something mobi2star does not support.',
      INCOMPLETE: 'The conversion stopped because a completeness check failed.',
      LIMIT: 'This dictionary is too large to convert in a browser.',
      VERIFY: 'The output failed its checks, so it was not offered for download.',
      IO: 'The file could not be read.',
      CRASH: 'The converter stopped unexpectedly, most likely because it ran out of memory.',
      LOAD: 'The converter could not be loaded. If this page was open while a new version was published, reload it and try again.',
      OTHER: 'The conversion failed.',
    },
  },
  zh: {
    ui: {
      toggle: 'English',
      toggleLabel: '将页面切换为英文',
      tagline: '在浏览器中把 MOBI 词典转换为 StarDict 格式。',
      privacy: '文件不会离开你的设备：转换完全在这个浏览器标签页里进行，不会上传任何内容。',
      choose: '选择一个 MOBI 词典，或拖放到这里',
      chosen: '已选择：{name}（{size}）',
      reader: '阅读器',
      labels: '附加标签的语言',
      labelsHint: '用于 mobi2star 为章节和图片集添加的查词键。',
      convert: '转换',
      cancel: '取消',
      cancelled: '已取消转换。',
      mobile: '大型词典需要较多内存，在手机和平板上可能转换失败。',
      done: '完成',
      checksPassed: '转换过程中的所有检查均已通过。',
      dictionary: '词典',
      headwords: '词头',
      aliases: '屈折形式与别名',
      entries: '索引条目',
      source: '转换来源',
      sourceSrcs: '内嵌的出版方源文件',
      sourceCompiled: '编译后的 MOBI 文本',
      download: '下载 {name}（{size}）',
      install: '安装',
      details: '检查内容（英文）',
      notes: '关于这些检查的说明（英文）',
      errorDetail: '详情：{detail}',
      cliHint: '命令行版本的限制更宽，检查也更多。',
      version: 'mobi2star {version}',
      sourceCode: '源代码',
      readerNotes: '阅读器说明',
      license: '请只转换你有权使用的词典。',
    },
    readers: {
      koreader: {
        name: 'KOReader',
        description: '电子阅读器和 Android 上的 KOReader，也适用于桌面版 GoldenDict。输出最小。',
        install: [
          '解压下载的文件。',
          '把文件夹复制到设备上 KOReader 目录中的 koreader/data/dict/。',
          '重启 KOReader。',
        ],
      },
      goldendict: {
        name: 'GoldenDict（桌面版）',
        description: '桌面版 GoldenDict、GoldenDict-ng 和 SilverDict。输出与 KOReader 相同。',
        install: [
          '解压下载的文件。',
          '在 GoldenDict 中打开 编辑 → 词典 → 词典来源 → 文件。',
          '添加该文件夹，然后点击重新扫描。',
        ],
      },
      'goldendict-mobile': {
        name: 'GoldenDict Mobile（Android）',
        description: 'Android 上的 GoldenDict Mobile。每个条目都带有一份样式表。',
        install: [
          '解压下载的文件。',
          '把文件夹复制到设备存储中的 GoldenDict 文件夹。',
          '在 GoldenDict 中重新扫描词典。',
        ],
      },
      readest: {
        name: 'Readest',
        description: 'Readest。每个条目都带有一份样式表。',
        install: [
          '解压下载的文件。',
          '在 Readest 的词典设置中导入该文件夹里的文件。',
        ],
      },
      kobo: {
        name: 'Kobo（经 PyGlossary 或 penelope）',
        description: '用于通过 PyGlossary 或 penelope 制作 Kobo 词典。每个条目都带有一份样式表。',
        install: [
          '解压下载的文件。',
          '用 PyGlossary 或 penelope 把该文件夹转换为 Kobo 词典。',
          '把结果复制到 Kobo 上的 .kobo/custom-dict/。',
        ],
      },
      universal: {
        name: '多个或其他阅读器',
        description: '同时用于多个阅读器，或未经测试的阅读器（如 Boox）。输出最大。',
        install: [
          '解压下载的文件。',
          '按各阅读器的方式添加该文件夹；可参考 KOReader 和 GoldenDict 的说明。',
        ],
      },
    },
    labels: {
      en: '英文',
      zh: '中文',
    },
    stages: {
      starting: '正在加载转换器…',
      parsing: '正在读取词典…',
      rendering: '正在转换条目：{done} / {total}',
      writing: '正在写入索引…',
      checking: '正在检查输出…',
    },
    errors: {
      MALFORMED: '这个文件不是 mobi2star 能读取的 MOBI 词典。',
      UNSUPPORTED: '这个词典用到了 mobi2star 尚不支持的内容。',
      INCOMPLETE: '完整性检查未通过，转换已停止。',
      LIMIT: '这个词典太大，无法在浏览器中转换。',
      VERIFY: '输出未通过检查，因此不提供下载。',
      IO: '无法读取该文件。',
      CRASH: '转换器意外停止，很可能是内存不足。',
      LOAD: '无法加载转换器。如果在新版本发布时这个页面一直开着，请刷新后重试。',
      OTHER: '转换失败。',
    },
  },
};

/** Replaces `{name}` placeholders in `template` with `values[name]`. */
export function format(template, values = {}) {
  return template.replace(/\{(\w+)\}/g, (match, key) => (key in values ? String(values[key]) : match));
}
