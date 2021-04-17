import requests

with open("test2_resized.jpg", "rb") as f:
    image = f.read()

with open("test2_thumb.jpg", "rb") as f:
    thumb = f.read()

base_url = "http://localhost:8000"

data = {
    "image": ('test2.jpg', image, 'image/jpeg'),
    "thumbnail": ('test2.jpg', thumb, 'image/jpeg')
}

request = requests.post(f"{base_url}/api/upload", files=data)
print(request.url)
print(request.text)