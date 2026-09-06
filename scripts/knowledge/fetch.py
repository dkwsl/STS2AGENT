#!/usr/bin/env python3
"""爬取杀戮尖塔2攻略，保存为 markdown 到 data/knowledge/raw/"""

import hashlib
import os
import re
import time
from pathlib import Path
from urllib.parse import urljoin

import requests
from bs4 import BeautifulSoup

HEADERS = {
    "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
    "Accept": "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
    "Accept-Language": "zh-CN,zh;q=0.9,en;q=0.8",
}

OUTPUT_DIR = Path(__file__).resolve().parent.parent.parent / "data" / "knowledge" / "raw"

# 攻略站 URL
STS2_SITE = "https://slaythespire-2.com/zh"

# 要爬取的子页面（关键词 → URL）
GUIDE_PAGES = {
    "角色总览": f"{STS2_SITE}/characters",
    "铁甲战士攻略": f"{STS2_SITE}/guides/ironclad-guide",
    "静默猎手攻略": f"{STS2_SITE}/guides/silent-guide",
    "故障机器人攻略": f"{STS2_SITE}/guides/defect-guide",
    "储君攻略": f"{STS2_SITE}/guides/regent-guide",
    "亡灵契师攻略": f"{STS2_SITE}/guides/necrobinder-guide",
    "新手攻略": f"{STS2_SITE}/guides/beginner-tips",
    "战斗机制": f"{STS2_SITE}/guides/combat-mechanics-guide",
    "最强构筑榜": f"{STS2_SITE}/guides/best-builds-tier-list",
    "铁甲战士卡牌强度": f"{STS2_SITE}/card-tier/ironclad",
    "静默猎手卡牌强度": f"{STS2_SITE}/card-tier/silent",
    "故障机器人卡牌强度": f"{STS2_SITE}/card-tier/defect",
    "储君卡牌强度": f"{STS2_SITE}/card-tier/regent",
    "亡灵契师卡牌强度": f"{STS2_SITE}/card-tier/necrobinder",
    "铁甲战士力量构筑": f"{STS2_SITE}/builds/ironclad-strength-build",
    "铁甲战士格挡构筑": f"{STS2_SITE}/builds/ironclad-barricade-build",
    "铁甲战士消耗构筑": f"{STS2_SITE}/builds/ironclad-exhaust-build",
    "角色概览": f"{STS2_SITE}/guides/character-overview",
}

def fetch_page(url):
    """下载网页 HTML"""
    resp = requests.get(url, headers=HEADERS, timeout=15)
    resp.encoding = resp.apparent_encoding or "utf-8"
    return resp.text

def html_to_markdown(html, base_url=""):
    """HTML → markdown，保留结构"""
    soup = BeautifulSoup(html, "html.parser")
    
    # 去掉无用标签
    for tag in soup.find_all(["script", "style", "nav", "footer", "header", "aside", "noscript", "iframe"]):
        tag.decompose()
    
    lines = []
    seen_texts = set()
    
    for el in soup.find_all(["h1", "h2", "h3", "h4", "h5", "h6", "p", "li", "td", "th"]):
        text = el.get_text(strip=True)
        if not text or len(text) < 2:
            continue
        # 去重
        if text in seen_texts:
            continue
        seen_texts.add(text)
        
        tag_name = el.name
        if tag_name == "h1":
            lines.append(f"\n# {text}\n")
        elif tag_name == "h2":
            lines.append(f"\n## {text}\n")
        elif tag_name == "h3":
            lines.append(f"\n### {text}\n")
        elif tag_name in ("h4", "h5", "h6"):
            lines.append(f"\n#### {text}\n")
        elif tag_name == "li":
            lines.append(f"- {text}")
        elif tag_name in ("td", "th"):
            lines.append(f"| {text} ")
        else:
            # p 标签：如果包含子元素已提取，跳过
            if el.find(["p", "li", "h1", "h2", "h3"]):
                continue
            lines.append(text)
    
    return "\n".join(lines)

def save_article(url, title, content, source=""):
    """保存为 markdown 文件"""
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    
    url_hash = hashlib.md5(url.encode()).hexdigest()[:8]
    safe_title = re.sub(r'[^\w\u4e00-\u9fff]', '_', title)[:30]
    source_tag = source or "web"
    filename = f"{source_tag}_{safe_title}_{url_hash}.md"
    filepath = OUTPUT_DIR / filename
    
    from datetime import datetime
    scrape_date = datetime.now().strftime("%Y-%m-%d")
    
    with open(filepath, "w", encoding="utf-8") as f:
        f.write(f"来源: {url}\n")
        f.write(f"标题: {title}\n")
        f.write(f"分类: {source}\n")
        f.write(f"爬取日期: {scrape_date}\n")
        f.write(f"游戏版本: 未知\n\n")
        f.write(content)
    
    print(f"  保存: {filepath.name} ({len(content)} 字)")
    return filepath

def crawl_all():
    """爬取所有预设页面"""
    print(f"开始爬取 {len(GUIDE_PAGES)} 个页面...")
    saved = 0
    failed = 0
    
    for name, url in GUIDE_PAGES.items():
        print(f"\n[{name}] {url}")
        try:
            html = fetch_page(url)
            content = html_to_markdown(html)
            
            # 提取标题
            soup = BeautifulSoup(html, "html.parser")
            title_tag = soup.find("title")
            title = title_tag.get_text(strip=True) if title_tag else name
            
            if len(content) < 100:
                print(f"  内容太短，跳过")
                failed += 1
                continue
            
            save_article(url, title, content, source="sts2site")
            saved += 1
            time.sleep(0.5)  # 间隔
            
        except Exception as e:
            print(f"  失败: {e}")
            failed += 1
    
    print(f"\n完成: 成功 {saved} 篇, 失败 {failed} 篇")
    print(f"保存目录: {OUTPUT_DIR}")

if __name__ == "__main__":
    crawl_all()
